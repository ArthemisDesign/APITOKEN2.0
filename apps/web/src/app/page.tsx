import type { Metadata } from "next";
import Link from "next/link";
import { DOCS_URL } from "@/lib/site-links";
import { JsonLd } from "@/components/json-ld";
import { B2C_DISCOUNT_PERCENT, officialUsageForTopup } from "@/lib/pricing-tiers";
import { PricingOverview } from "@/components/pricing-overview";
import { CostCalculator } from "@/components/cost-calculator";
import { T } from "@/components/translated";
import { absoluteUrl, createPageMetadata, SITE_NAME, SITE_ORIGIN, seoPages } from "@/lib/seo";
import { coreAlternates } from "@/lib/seo-core";

export const metadata: Metadata = {
  ...createPageMetadata(seoPages.home, { absoluteTitle: `${seoPages.home.title} | ${SITE_NAME}` }),
  alternates: coreAlternates("/"),
};

// The English canonical route does not need the locale request header. Keeping
// it static lets Vercel serve the landing HTML from the edge cache.
export const dynamic = "force-static";

// Hero-значки: чистые inline-SVG (accent stroke), одинаково резкие в обеих темах — крупнее и контрастнее PNG.
const heroIcons = {
  connect: <><path d="M12 22v-5" /><path d="M9 8V2" /><path d="M15 8V2" /><path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z" /></>,
  product: <><path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z" /><path d="m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65" /><path d="m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65" /></>,
  noaccount: <><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" /><circle cx="9" cy="7" r="4" /><path d="m17 8 5 5" /><path d="m22 8-5 5" /></>,
  novpn: <><circle cx="12" cy="12" r="10" /><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" /><path d="M2 12h20" /></>,
};
const heroPoints = [
  { k: "hero_p1", en: "Integration with Claude Code, Codex, Cursor, opencode & more", ic: "connect" },
  { k: "hero_p2", en: "Add it to any product or workflow", ic: "product" },
  { k: "hero_p3", en: "No Anthropic or OpenAI account", ic: "noaccount" },
  { k: "hero_p4", en: "No VPN", ic: "novpn" },
] as const;
// Единый developer-flow: три шага, каждый несёт реально важное (баланс/модели → drop-in эндпоинт → продакшн-биллинг).
const flow = [
  { num: "01", h: "step1_h", p: "step1_p", raw: [] as string[], chips: ["chip_models", "chip_tiers", "chip_balance"] },
  { num: "02", h: "step2_h", p: "step2_p", raw: ["Claude Code", "Codex", "Cursor", "Cline", "opencode", "SDK"], chips: [] as string[] },
  { num: "03", h: "step3_h", p: "step3_p", raw: [] as string[], chips: ["chip_stream", "chip_limits", "chip_usage"] },
] as const;

const faqItems = [
  { question: "q1", answer: "a1", q: "How much does API access cost?", a: "Enter any positive whole USD top-up amount. Each request is converted to official API spend, then your active discount is applied: B2C takes a flat 50% off every request, while B2B pricing is negotiated." },
  { question: "q2", answer: "a2", q: "Is there a free option?", a: "Yes — new B2C accounts created with Google or GitHub get $10 of API usage at official prices, valid on Claude and GPT models. Email and password accounts are not eligible." },
  { question: "q3", answer: "a3", q: "Which models are available?", a: "The Claude line — Opus 4.8, Opus 4.7, Sonnet 5, Sonnet 4.6 and Haiku 4.5 — plus the GPT line — GPT-5.6 Sol, Terra and Luna, GPT-5.5 and GPT-5.4. One balance and one API key cover every model." },
  { question: "q4", answer: "a4", q: "Which model is best for coding?", a: "For agentic coding and long sessions, Claude Opus and Sonnet and GPT-5.6 Sol give the best results; Claude Haiku and GPT-5.6 Luna are ideal for fast, cheap calls." },
  { question: "q5", answer: "a5", q: "What base URL should I use?", a: "For Claude and Anthropic-compatible tools use https://api.apitoken.sale (POST /v1/messages, x-api-key). For GPT and OpenAI-compatible tools use https://openai.api.apitoken.sale/v1 (Responses and Chat Completions, Authorization: Bearer). Both draw on the same key and balance." },
  { question: "q6", answer: "a6", q: "What happens if I receive a 429?", a: "Honor Retry-After and reduce concurrency. Dashboard key guardrails are a lifetime spending limit and an optional expiration date; they do not configure request throughput." },
] as const;

const homeJsonLd = {
  "@context": "https://schema.org",
  "@graph": [
    {
      "@type": "Organization",
      "@id": `${SITE_ORIGIN}/#organization`,
      name: SITE_NAME,
      alternateName: ["API Token Sale", "apitoken.sale"],
      url: SITE_ORIGIN,
      logo: {
        "@type": "ImageObject",
        url: absoluteUrl("/assets/favicon-512.png"),
        width: 512,
        height: 512,
      },
      email: "apitokensale@gmail.com",
      sameAs: ["https://t.me/apitokensupportbot"],
      contactPoint: {
        "@type": "ContactPoint",
        contactType: "customer support",
        email: "apitokensale@gmail.com",
        url: absoluteUrl("/contacts"),
        availableLanguage: ["English", "Russian"],
      },
    },
    {
      "@type": "WebSite",
      "@id": `${SITE_ORIGIN}/#website`,
      url: SITE_ORIGIN,
      name: SITE_NAME,
      alternateName: "apiToken.sale Claude & GPT API",
      description: seoPages.home.description,
      inLanguage: "en",
      publisher: { "@id": `${SITE_ORIGIN}/#organization` },
    },
    {
      "@type": "Service",
      "@id": `${SITE_ORIGIN}/#service`,
      name: "Claude and GPT API access for developers",
      description: seoPages.home.description,
      url: SITE_ORIGIN,
      serviceType: "Anthropic-compatible and OpenAI-compatible API access",
      areaServed: "Worldwide",
      audience: { "@type": "Audience", audienceType: "Software developers" },
      provider: { "@id": `${SITE_ORIGIN}/#organization` },
      termsOfService: absoluteUrl("/terms"),
    },
    {
      "@type": "FAQPage",
      "@id": `${SITE_ORIGIN}/#faq`,
      mainEntity: faqItems.map((item) => ({
        "@type": "Question",
        name: item.q,
        acceptedAnswer: { "@type": "Answer", text: item.a },
      })),
    },
  ],
};

export default function HomePage() {
  return <><JsonLd data={homeJsonLd} /><main>
      <div className="hero"><div className="wrap hero-grid"><div><T k="hero_eyebrow" as="span" className="eyebrow">Claude API · One gateway</T><h1 className="hero-h1"><T k="hero_h1a" as="span" className="hero-h1-main">Buy Claude API access</T><T k="hero_h1b" as="span" className="hero-sub">Same as official, but cheaper</T></h1><ul className="hero-points">{heroPoints.map((point) => <li key={point.k}><span className="hp-ic" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round">{heroIcons[point.ic]}</svg></span><T k={point.k}>{point.en}</T></li>)}</ul><div className="hero-cta"><Link className="btn btn-primary" href="/register"><T k="hero_cta1">Get API key</T></Link><Link className="btn btn-ghost" href={DOCS_URL} target="_blank" rel="noreferrer"><T k="hero_cta2">Read documentation</T></Link></div></div><HeroDiscount /></div><div className="wrap home-stats"><div className="stats reveal"><Stat value="10+" label="stat1" /><Stat value="3" label="stat2" /><Stat value="50%" label="stat3" /><Stat value="2" label="stat4" /><div className="stat"><T k="stat5v" as="b">minutes</T><T k="stat5">Setup time</T></div></div></div></div>
      <section id="products"><div className="wrap"><SectionHead eyebrow="prod_eyebrow" title="prod_h2" lead="prod_lead" />
        <div className="home-calc" data-reveal><CostCalculator /><p className="home-calc-more"><Link href="/tools/claude-api-cost-calculator"><T k="home_calc_more">Full calculator — prompt caching, every model &amp; pricing FAQ →</T></Link></p></div>
      </div></section>
      <section id="how"><div className="wrap"><SectionHead eyebrow="how_eyebrow" title="how_h2" lead="how_lead" />
        <div className="flow" data-reveal-stagger>{flow.map((step) => <article key={step.num} className="flow-step">
          <div className="flow-top"><span className="n">{step.num}</span></div>
          <T k={step.h} as="h3">Step</T>
          <T k={step.p} as="p">Detail</T>
          <ul className="flow-chips">{step.raw.map((chip) => <li key={chip}>{chip}</li>)}{step.chips.map((chip) => <li key={chip}><T k={chip}>{chip}</T></li>)}</ul>
        </article>)}</div>
      </div></section>
      <section id="pricing"><div className="wrap"><SectionHead eyebrow="pr_eyebrow" title="pr_h2" lead="pr_lead" /><PricingOverview /></div></section>
      <section id="faq"><div className="wrap"><SectionHead eyebrow="faq_eyebrow" title="faq_h2" lead="faq_lead" />
        <div className="faq">{faqItems.map((item) => <details key={item.question}>
          <summary><T k={item.question}>{item.q}</T><span className="plus" aria-hidden="true">＋</span></summary>
          <T k={item.answer} as="p" className="ans">{item.a}</T>
        </details>)}</div>
      </div></section>
      <section className="cta-band"><div className="wrap cta-row reveal">
        <T k="cta_h2" as="h2">Ready to start building?</T>
        <div className="cta-actions">
          <Link className="btn btn-primary" href="/register"><T k="hero_cta1">Get API key</T></Link>
          <Link className="btn btn-ghost" href={DOCS_URL} target="_blank" rel="noreferrer"><T k="hero_cta2">Read documentation</T></Link>
        </div>
      </div></section>
    </main></>;
}

// Сколько официального Claude API получит клиент за пополнение `payUsd`: плоская скидка 50% → номинал ×2.
function pricingForTopup(payUsd: number) {
  return { pay: payUsd, get: Math.round(officialUsageForTopup(payUsd)), discount: B2C_DISCOUNT_PERCENT };
}
const heroTopups = [10, 100, 1000].map(pricingForTopup);

function HeroDiscount() {
  return <aside className="hero-offer reveal">
    <div className="offer-free">
      <div className="offer-free-top">
        <T k="offer_free_eyebrow" as="span" className="offer-kicker">Welcome credit</T>
        <T k="offer_no_card" as="span" className="offer-no-card">No card required</T>
      </div>
      <h2 className="offer-free-head"><span className="ofa">$10</span><T k="offer_free_label" as="span" className="off-tag">Free</T></h2>
      <T k="offer_free_sub" as="p" className="offer-free-sub">Claude API usage at official prices</T>
      <T k="offer_free_note" as="p" className="offer-free-note">For new accounts created with Google or GitHub</T>
      <T k="offer_free_terms" as="p" className="offer-free-terms">Granted after automated anti-fraud screening; may be withheld for duplicate or linked accounts.</T>
    </div>
    <div className="offer-rate-head">
      <T k="offer_note" as="h3">Your balance goes further</T>
      <T k="offer_note_detail" as="p">Flat 50% B2C discount on every request</T>
    </div>
    <div className="offer-value-table">
      <div className="offer-table-head">
        <T k="offer_top_up" as="span">Top up</T>
        <T k="offer_discount" as="span">Discount</T>
        <T k="offer_in_api" as="span">Claude API value</T>
      </div>
      <ul className="offer-tiers">{heroTopups.map((row, index) => <li key={row.pay} className={index === heroTopups.length - 1 ? "ot ot-best" : "ot"}>
        <span className="ot-pay"><b>${row.pay.toLocaleString("en-US")}</b></span>
        <span className="ot-rate"><i>−{row.discount}%</i><span aria-hidden="true">→</span></span>
        <span className="ot-get"><b>${row.get.toLocaleString("en-US")}</b></span>
      </li>)}</ul>
    </div>
  </aside>;
}
function Stat({ value, label }: { value: string; label: string }) { return <div className="stat"><b>{value}</b><T k={label}>Metric</T></div>; }
function SectionHead({ eyebrow, title, lead }: { eyebrow: string; title: string; lead?: string }) { return <div className="sec-head reveal"><T k={eyebrow} as="span" className="eyebrow">Section</T><T k={title} as="h2">Title</T>{lead && <T k={lead} as="p">Description</T>}</div>; }
