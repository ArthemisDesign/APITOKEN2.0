import Link from "next/link";
import { DOCS_URL } from "@/lib/site-links";
import { InteractiveTerminal } from "@/components/interactive-terminal";
import { PricingOverview } from "@/components/pricing-overview";
import { TopUpAmountInput } from "@/components/topup-amount-input";
import { T } from "@/components/translated";

const models = ["Claude Opus 4.8", "Claude Opus 4.7", "Claude Sonnet 4.6", "Claude Haiku 4.5"];
// Единый developer-flow: три шага, каждый несёт реально важное (баланс/модели → drop-in эндпоинт → продакшн-биллинг).
const flow = [
  { num: "01", h: "step1_h", p: "step1_p", raw: [] as string[], chips: ["chip_models", "chip_tiers", "chip_balance"] },
  { num: "02", h: "step2_h", p: "step2_p", raw: ["Claude Code", "Cursor", "Cline", "Continue", "OpenClaw", "SDK"], chips: [] as string[] },
  { num: "03", h: "step3_h", p: "step3_p", raw: [] as string[], chips: ["chip_stream", "chip_limits", "chip_usage"] },
] as const;

export default function HomePage() {
  return <main>
      <div className="hero"><div className="wrap hero-grid"><div><T k="hero_eyebrow" as="span" className="eyebrow">Claude API · One gateway</T><h1 className="hero-h1"><T k="hero_h1a" as="span">All Claude models.</T><T k="hero_h1b" as="span">One key.</T></h1><ul className="hero-points"><li><T k="hero_p1">Drop into Claude Code, Cursor, OpenClaw — any tool that needs a key</T></li><li><T k="hero_p2">Add it to any product or workflow</T></li><li><T k="hero_p3">No Anthropic account required</T></li><li><T k="hero_p4">No VPN — works in any country</T></li></ul><div className="hero-cta"><Link className="btn btn-primary" href="/register"><T k="hero_cta1">Get API key</T></Link><Link className="btn btn-ghost" href={DOCS_URL} target="_blank" rel="noreferrer"><T k="hero_cta2">Read documentation</T></Link></div></div><InteractiveTerminal /></div><div className="wrap home-stats"><div className="stats reveal"><Stat value="8+" label="stat1" /><Stat value="1" label="stat2" /><Stat value="99.9%" label="stat3" /><Stat value="<100ms" label="stat4" /><div className="stat"><T k="stat5v" as="b">minutes</T><T k="stat5">Setup time</T></div></div></div></div>
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
          <T k="pc2_h" as="h3">Pay less than official</T>
          <div className="prod-body"><TopUpAmountInput className="amount-example" initialAmount="1000" showReceive /><T k="pc2_p" as="p">Top up any whole USD amount — spend it at official Anthropic prices minus up to 80%.</T></div>
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
      <section className="cta-band"><div className="wrap cta-row reveal">
        <T k="cta_h2" as="h2">Ready to start building?</T>
        <div className="cta-actions">
          <Link className="btn btn-primary" href="/register"><T k="hero_cta1">Get API key</T></Link>
          <Link className="btn btn-ghost" href={DOCS_URL} target="_blank" rel="noreferrer"><T k="hero_cta2">Read documentation</T></Link>
        </div>
      </div></section>
    </main>;
}

function Stat({ value, label }: { value: string; label: string }) { return <div className="stat"><b>{value}</b><T k={label}>Metric</T></div>; }
function SectionHead({ eyebrow, title, lead }: { eyebrow: string; title: string; lead?: string }) { return <div className="sec-head reveal"><T k={eyebrow} as="span" className="eyebrow">Section</T><T k={title} as="h2">Title</T>{lead && <T k={lead} as="p">Description</T>}</div>; }
