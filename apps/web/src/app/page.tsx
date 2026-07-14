import Link from "next/link";
import { InteractiveTerminal } from "@/components/interactive-terminal";
import { MotionEffects } from "@/components/motion-effects";
import { PricingOverview } from "@/components/pricing-overview";
import { SiteFooter, SiteHeader } from "@/components/site-chrome";
import { TopUpAmountInput } from "@/components/topup-amount-input";
import { T } from "@/components/translated";

const models = ["Claude Opus 4.8", "Claude Opus 4.7", "Claude Sonnet 4.6", "Claude Haiku 4.5"];
const steps = [["step1_h","step1_p"],["step2_h","step2_p"],["step3_h","step3_p"]] as const;
const features = [["f1_h","f1_p"],["f2_h","f2_p"],["f3_h","f3_p"],["f4_h","f4_p"]] as const;
const meters = [["mt1_h","mt1_p"],["mt2_h","mt2_p"],["mt3_h","mt3_p"],["mt4_h","mt4_p"],["mt5_h","mt5_p"]] as const;
const faqs = [["q1","a1"],["q2","a2"],["q3","a3"],["q4","a4"],["q5","a5"],["q6","a6"]] as const;

export default function HomePage() {
  return <>
    <SiteHeader home />
    <main>
      <div className="hero"><div className="wrap hero-grid"><div><T k="hero_eyebrow" as="span" className="eyebrow">Claude API · One gateway</T><T k="hero_h1" as="h1">All Claude models. One key.</T><T k="hero_lead" as="p" className="lead">One key, one dashboard, every Claude model — ready for Claude Code, Cursor and production.</T><div className="hero-cta"><Link className="btn btn-primary" href="/register"><T k="hero_cta1">Get API key</T></Link><Link className="btn btn-ghost" href="/docs"><T k="hero_cta2">Read documentation</T></Link></div><T k="hero_note" as="p" className="hero-note">New accounts get $2.50 in free test usage.</T></div><InteractiveTerminal /></div><div className="wrap home-stats"><div className="stats reveal"><Stat value="8+" label="stat1" /><Stat value="2" label="stat2" /><Stat value="99.9%" label="stat3" /><Stat value="<100ms" label="stat4" /><div className="stat"><T k="stat5v" as="b">minutes</T><T k="stat5">Setup time</T></div></div></div></div>
      <section id="products"><div className="wrap"><div className="prod-grid" data-reveal-stagger><div className="prod"><T k="pc1_tag" as="span" className="tag">Claude API</T><T k="pc1_h" as="h3">All models, one key</T><ul className="prod-list">{models.map((model) => <li key={model}>{model}</li>)}</ul><Link className="btn btn-ghost" href="/models"><T k="pc1_cta">View models</T></Link></div><div className="prod prod-feat"><T k="pc2_tag" as="span" className="tag">Flexible balance</T><T k="pc2_h" as="h3">Choose your amount</T><TopUpAmountInput className="amount-example" initialAmount="37" /><T k="pc2_p" as="p">Enter any whole USD amount. No fixed product catalog.</T><Link className="btn btn-primary" href="#pricing"><T k="pc2_cta">See pricing</T></Link></div><div className="prod"><T k="pc3_tag" as="span" className="tag">Free start</T><T k="pc3_h" as="h3">Free test balance</T><div className="amt"><T k="p3_now" as="span" className="now">Free</T><span className="was">$2.50</span></div><T k="pc3_p" as="p">Every new account gets $2.50 in test usage. No card required.</T><Link className="btn btn-ghost" href="/register"><T k="start_free">Start free</T></Link></div></div></div></section>
      <section id="how"><div className="wrap"><SectionHead eyebrow="how_eyebrow" title="how_h2" lead="how_lead" /><div className="steps" data-reveal-stagger>{steps.map(([title, text], index) => <InfoCard key={title} index={index} title={title} text={text} className="step" />)}</div></div></section>
      <section id="workflow"><div className="wrap"><SectionHead eyebrow="wf_eyebrow" title="wf_h2" /><div className="feats" data-reveal-stagger>{features.map(([title, text], index) => <InfoCard key={title} index={index} title={title} text={text} className="feat" />)}</div></div></section>
      <section className="announce"><div className="wrap announce-inner reveal"><div className="announce-copy"><T k="ann_eyebrow" as="span" className="eyebrow">Latest model</T><T k="ann_h" as="h2">Claude Opus 4.8 is live</T><T k="ann_p" as="p">The most capable Claude model runs on the same key and balance.</T><Link className="btn btn-ghost" href="/models"><T k="ann_cta">Explore models</T></Link></div><div className="announce-badge"><b>Opus 4.8</b><T k="ann_ctx">1M context</T></div></div></section>
      <section id="pricing"><div className="wrap"><SectionHead eyebrow="pr_eyebrow" title="pr_h2" lead="pr_lead" /><PricingOverview /><div className="tokens" data-reveal-stagger><T k="billing_label">Billing basis:</T>{[1,2,3,4,5].map((index) => <T k={`bill${index}`} key={index}>Billing step</T>)}</div></div></section>
      <section id="metered"><div className="wrap"><SectionHead eyebrow="mt_eyebrow" title="mt_h2" lead="mt_lead" /><div className="meters" data-reveal-stagger>{meters.map(([title,text], index) => <InfoCard key={title} index={index} title={title} text={text} className="meter" />)}</div></div></section>
      <section id="faq"><div className="wrap"><SectionHead eyebrow="faq_eyebrow" title="faq_h2" lead="faq_lead" /><div className="faq" data-reveal-stagger>{faqs.map(([question, answer]) => <details key={question}><summary><T k={question}>Question</T><span className="plus">+</span></summary><T k={answer} as="div" className="ans">Answer</T></details>)}</div></div></section>
      <section className="cta-band"><div className="wrap reveal"><T k="cta_h2" as="h2">Ready to start building?</T><T k="cta_p" as="p">Create a key in minutes.</T><div className="cta-actions"><Link className="btn btn-primary" href="/register"><T k="hero_cta1">Get API key</T></Link><Link className="btn btn-ghost" href="/docs"><T k="hero_cta2">Read documentation</T></Link></div></div></section>
    </main>
    <SiteFooter full />
    <MotionEffects />
  </>;
}

function Stat({ value, label }: { value: string; label: string }) { return <div className="stat"><b>{value}</b><T k={label}>Metric</T></div>; }
function SectionHead({ eyebrow, title, lead }: { eyebrow: string; title: string; lead?: string }) { return <div className="sec-head reveal"><T k={eyebrow} as="span" className="eyebrow">Section</T><T k={title} as="h2">Title</T>{lead && <T k={lead} as="p">Description</T>}</div>; }
function InfoCard({ index, title, text, className }: { index: number; title: string; text: string; className: string }) { return <div className={className}><div className="n">{String(index + 1).padStart(2,"0")}</div><T k={title} as="h3">Title</T><T k={text} as="p">Description</T></div>; }
