import Link from "next/link";
import { DOCS_URL } from "@/lib/site-links";
import { InteractiveTerminal } from "@/components/interactive-terminal";
import { PricingOverview } from "@/components/pricing-overview";
import { TopUpAmountInput } from "@/components/topup-amount-input";
import { T } from "@/components/translated";

const models = ["Claude Opus 4.8", "Claude Opus 4.7", "Claude Sonnet 4.6", "Claude Haiku 4.5"];
const steps = [["step1_h","step1_p"],["step2_h","step2_p"],["step3_h","step3_p"]] as const;
const features = [["f1_h","f1_p"],["f2_h","f2_p"],["f3_h","f3_p"],["f4_h","f4_p"]] as const;

export default function HomePage() {
  return <main>
      <div className="hero"><div className="wrap hero-grid"><div><T k="hero_eyebrow" as="span" className="eyebrow">Claude API · One gateway</T><T k="hero_h1" as="h1">All Claude models. One key.</T><T k="hero_lead" as="p" className="lead">One key, one dashboard, every Claude model — ready for Claude Code, Cursor and production.</T><div className="hero-cta"><Link className="btn btn-primary" href="/register"><T k="hero_cta1">Get API key</T></Link><Link className="btn btn-ghost" href={DOCS_URL} target="_blank" rel="noreferrer"><T k="hero_cta2">Read documentation</T></Link></div></div><InteractiveTerminal /></div><div className="wrap home-stats"><div className="stats reveal"><Stat value="8+" label="stat1" /><Stat value="1" label="stat2" /><Stat value="99.9%" label="stat3" /><Stat value="<100ms" label="stat4" /><div className="stat"><T k="stat5v" as="b">minutes</T><T k="stat5">Setup time</T></div></div></div></div>
      <section id="products"><div className="wrap"><div className="prod-grid" data-reveal-stagger>
        <div className="prod">
          <T k="pc1_tag" as="span" className="tag">Claude API</T>
          <T k="pc1_h" as="h3">All models, one key</T>
          <div className="prod-body"><ul className="prod-list">{models.map((model) => <li key={model}>{model}</li>)}</ul></div>
          <Link className="btn btn-ghost" href="/models"><T k="pc1_cta">View models</T></Link>
        </div>
        <div className="prod prod-feat">
          <T k="pc2_tag" as="span" className="tag">Flexible balance</T>
          <T k="pc2_h" as="h3">Choose your amount</T>
          <div className="prod-body"><TopUpAmountInput className="amount-example" initialAmount="1000" showReceive /><T k="pc2_p" as="p">Enter any whole USD amount. No fixed product catalog.</T></div>
          <Link className="btn btn-primary" href="#pricing"><T k="pc2_cta">See pricing</T></Link>
        </div>
        <div className="prod">
          <T k="pc3_tag" as="span" className="tag">Free start</T>
          <T k="pc3_h" as="h3">Free API usage</T>
          <div className="prod-body"><div className="amt"><T k="p3_now" as="span" className="now">$10</T><T k="pc3_official" as="span" className="official">at official API prices</T></div><T k="pc3_p" as="p">No card required. Start with any Claude model.</T></div>
          <Link className="btn btn-ghost" href="/register"><T k="start_free">Start free</T></Link>
        </div>
      </div></div></section>
      <section id="how"><div className="wrap"><SectionHead eyebrow="how_eyebrow" title="how_h2" lead="how_lead" /><div className="steps" data-reveal-stagger>{steps.map(([title, text], index) => <InfoCard key={title} index={index} title={title} text={text} className="step" />)}</div></div></section>
      <section id="workflow"><div className="wrap"><SectionHead eyebrow="wf_eyebrow" title="wf_h2" /><div className="feats" data-reveal-stagger>{features.map(([title, text], index) => <InfoCard key={title} index={index} title={title} text={text} className="feat" />)}</div></div></section>
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
function InfoCard({ index, title, text, className }: { index: number; title: string; text: string; className: string }) { return <div className={className}><div className="n">{String(index + 1).padStart(2,"0")}</div><T k={title} as="h3">Title</T><T k={text} as="p">Description</T></div>; }
