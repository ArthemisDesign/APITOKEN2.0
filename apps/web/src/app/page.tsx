import type { Metadata } from "next";
import Image from "next/image";
import Link from "next/link";
import { DOCS_URL } from "@/lib/site-links";
import { JsonLd } from "@/components/json-ld";
import { B2C_PRICING_MILESTONES, formatWholeUsd } from "@/lib/pricing-tiers";
import { PricingOverview } from "@/components/pricing-overview";
import { TopUpAmountInput } from "@/components/topup-amount-input";
import { T } from "@/components/translated";
import { absoluteUrl, createPageMetadata, SITE_NAME, SITE_ORIGIN, seoPages } from "@/lib/seo";

export const metadata: Metadata = createPageMetadata(seoPages.home, {
  absoluteTitle: `${seoPages.home.title} | ${SITE_NAME}`,
});

const models = ["Claude Opus 4.8", "Claude Opus 4.7", "Claude Sonnet 5", "Claude Sonnet 4.6", "Claude Haiku 4.5"];
// Hero-тезисы: у каждого свой пиксель-значок (свой файл на светлую/тёмную тему).
const heroPoints = [
  { k: "hero_p1", en: "Integration with Claude Code, Cursor, OpenClaw & more", ic: "connect" },
  { k: "hero_p2", en: "Add it to any product or workflow", ic: "product" },
  { k: "hero_p3", en: "No Anthropic account", ic: "noaccount" },
  { k: "hero_p4", en: "No VPN", ic: "novpn" },
] as const;
// Единый developer-flow: три шага, каждый несёт реально важное (баланс/модели → drop-in эндпоинт → продакшн-биллинг).
const flow = [
  { num: "01", h: "step1_h", p: "step1_p", raw: [] as string[], chips: ["chip_models", "chip_tiers", "chip_balance"] },
  { num: "02", h: "step2_h", p: "step2_p", raw: ["Claude Code", "Cursor", "Cline", "Continue", "OpenClaw", "SDK"], chips: [] as string[] },
  { num: "03", h: "step3_h", p: "step3_p", raw: [] as string[], chips: ["chip_stream", "chip_limits", "chip_usage"] },
] as const;

const faqItems = [
  { question: "q1", answer: "a1", q: "How much does the Claude API cost?", a: "Enter any positive whole USD top-up amount. Each request is converted to official API spend, then your active discount is applied: B2C progresses from 60% to 80% off, while B2B pricing is negotiated." },
  { question: "q2", answer: "a2", q: "Is there a free option?", a: "Yes — every new B2C account gets $10 of Claude usage at official API prices, enough to wire up your tools and run real calls." },
  { question: "q3", answer: "a3", q: "Which models are available?", a: "The current supported Claude line includes Opus, Sonnet, and Haiku models on the same balance and API key." },
  { question: "q4", answer: "a4", q: "Which Claude model is best for coding?", a: "For agentic coding and long sessions, Opus and Sonnet tiers give the best results; Haiku is ideal for fast, economical calls." },
  { question: "q5", answer: "a5", q: "What base URL should I use?", a: "Use https://api.apitoken.sale with any Anthropic-compatible tool. Send requests through /v1/messages with the same key and balance." },
  { question: "q6", answer: "a6", q: "What are the rate limits?", a: "Per-key request caps are configurable in the dashboard, so you can protect your balance and split limits across tools." },
] as const;

const homeJsonLd = {
  "@context": "https://schema.org",
  "@graph": [
    {
      "@type": "Organization",
      "@id": `${SITE_ORIGIN}/#organization`,
      name: SITE_NAME,
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
      alternateName: "apiToken.sale Claude API",
      description: seoPages.home.description,
      inLanguage: "en",
      publisher: { "@id": `${SITE_ORIGIN}/#organization` },
    },
    {
      "@type": "Service",
      "@id": `${SITE_ORIGIN}/#service`,
      name: "Claude API access for developers",
      description: seoPages.home.description,
      url: SITE_ORIGIN,
      serviceType: "Anthropic-compatible API access",
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
      <div className="hero"><div className="wrap hero-grid"><div><T k="hero_eyebrow" as="span" className="eyebrow">Claude API · One gateway</T><h1 className="hero-h1"><T k="hero_h1a" as="span" className="hero-h1-main">Buy Claude API access</T><T k="hero_h1b" as="span" className="hero-sub">Same as official, but cheaper</T></h1><ul className="hero-points">{heroPoints.map((point) => <li key={point.k}><span className="hp-ic" aria-hidden="true"><Image className="hp-ic-l" src={`/assets/feat-${point.ic}-light.png`} width={30} height={30} alt="" /><Image className="hp-ic-d" src={`/assets/feat-${point.ic}-dark.png`} width={30} height={30} alt="" /></span><T k={point.k}>{point.en}</T></li>)}</ul><div className="hero-cta"><Link className="btn btn-primary" href="/register"><T k="hero_cta1">Get API key</T></Link><Link className="btn btn-ghost" href={DOCS_URL} target="_blank" rel="noreferrer"><T k="hero_cta2">Read documentation</T></Link></div></div><HeroDiscount /></div><div className="wrap home-stats"><div className="stats reveal"><Stat value="8+" label="stat1" /><Stat value="1" label="stat2" /><Stat value="99.9%" label="stat3" /><Stat value="<100ms" label="stat4" /><div className="stat"><T k="stat5v" as="b">minutes</T><T k="stat5">Setup time</T></div></div></div></div>
      <section id="products"><div className="wrap"><SectionHead eyebrow="prod_eyebrow" title="prod_h2" lead="prod_lead" />
        <div className="prod-grid" data-reveal-stagger>
        <div className="prod">
          <T k="pc1_tag" as="span" className="tag">Official Anthropic API</T>
          <T k="pc1_h" as="h3">All models, one key</T>
          <div className="prod-body">
            <T k="pc1_note" as="p" className="prod-note">The same Anthropic Messages API and the full Claude line — billed at your discount.</T>
            <ul className="prod-chips">{models.map((model) => <li key={model}>{model}</li>)}</ul>
          </div>
          <Link className="btn btn-ghost" href="/models"><T k="pc1_cta">View models</T></Link>
        </div>
        <div className="prod prod-feat">
          <T k="pc2_tag" as="span" className="tag">Your discount</T>
          <T k="pc2_h" as="h3">The same API, cheaper</T>
          <div className="prod-body"><TopUpAmountInput className="amount-example" initialAmount="1000" showReceive /><T k="pc2_p" as="p">The exact same Anthropic API — same models, same endpoints, same responses. You just pay up to 80% less per call.</T></div>
          <Link className="btn btn-primary" href="#pricing"><T k="pc2_cta">See pricing</T></Link>
        </div>
        <div className="prod">
          <T k="pc3_tag" as="span" className="tag">Free start</T>
          <T k="pc3_h" as="h3">Free API usage</T>
          <div className="prod-body"><div className="amt"><T k="p3_now" as="span" className="now">$10</T><T k="pc3_official" as="span" className="official">at official API prices</T></div><T k="pc3_p" as="p">No card required. Start with any Claude model.</T></div>
          <Link className="btn btn-ghost" href="/register"><T k="start_free">Start free</T></Link>
        </div>
      </div></div></section>
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

function HeroDiscount() {
  return <aside className="hero-offer reveal" aria-label="Discount by top-up">
    <div className="offer-free">
      <span className="offer-free-badge"><T k="offer_free_badge">FREE</T></span>
      <span className="offer-free-amt">$10</span>
      <T k="offer_free_note" as="span" className="offer-free-note">on signup — no card</T>
    </div>
    <div className="offer-head"><T k="offer_col_topup" as="span">Top up</T><T k="offer_col_off" as="span">You save</T></div>
    <ul className="offer-rows">{B2C_PRICING_MILESTONES.map((tier) => <li key={tier.code}>
      <span className="offer-amt">{Number(tier.platformSpendUsd) === 0 ? <T k="offer_free_start">Free</T> : formatWholeUsd(tier.platformSpendUsd)}</span>
      <span className="offer-pct"><b>{tier.discountPercent}</b><i>% off</i></span>
    </li>)}</ul>
    <T k="offer_foot" as="p" className="offer-foot">Discount locks in and applies to every call.</T>
  </aside>;
}
function Stat({ value, label }: { value: string; label: string }) { return <div className="stat"><b>{value}</b><T k={label}>Metric</T></div>; }
function SectionHead({ eyebrow, title, lead }: { eyebrow: string; title: string; lead?: string }) { return <div className="sec-head reveal"><T k={eyebrow} as="span" className="eyebrow">Section</T><T k={title} as="h2">Title</T>{lead && <T k={lead} as="p">Description</T>}</div>; }
