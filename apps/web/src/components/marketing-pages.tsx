import Link from "next/link";
import type { ReactNode } from "react";
import { MotionEffects } from "./motion-effects";
import { PricingOverview } from "./pricing-overview";
import { SiteFooter, SiteHeader } from "./site-chrome";
import { T } from "./translated";

export function MarketingFrame({ children }: { children: ReactNode }) {
  return <><SiteHeader /><main>{children}</main><SiteFooter /><MotionEffects /></>;
}

export function PageHero({ eyebrow, title, subtitle, back }: { eyebrow: string; title: string; subtitle: string; back?: boolean }) {
  return <div className="page-hero"><div className="wrap">{back && <Link className="auth-back" href="/integrations"><T k="int_back">← All integrations</T></Link>}<T k={eyebrow} as="span" className="eyebrow">Section</T><T k={title} as="h1">Title</T><T k={subtitle} as="p">Description</T></div></div>;
}

export function PlansPage() {
  return <MarketingFrame><PageHero eyebrow="pr_eyebrow" title="plans_h" subtitle="plans_sub" /><section className="borderless"><div className="wrap"><PricingOverview /><div className="tokens page-tokens"><T k="tokens_label">Metered per token:</T>{[1,2,3,4,5].map((index) => <T k={`tk${index}`} key={index}>Token type</T>)}</div></div></section></MarketingFrame>;
}

const modelRows = [
  ["Claude Opus 4.8","claude-opus-4-8","1M","$5","$25","m_opus48"],
  ["Claude Opus 4.7","claude-opus-4-7","1M","$5","$25","m_opus47"],
  ["Claude Sonnet 4.6","claude-sonnet-4-6","1M","$3","$15","m_son46"],
  ["Claude Haiku 4.5","claude-haiku-4-5","200K","$1","$5","m_haiku"],
] as const;

export function ModelsPage() {
  return <MarketingFrame><PageHero eyebrow="nav_models" title="models_h" subtitle="models_sub" /><section className="borderless"><div className="wrap"><div className="model-rate-note"><div><T k="model_rate_tag" as="span" className="tag">Official list rates</T><T k="model_rate_h" as="h3">Transparent per-token reference pricing</T></div><T k="model_rate_p" as="p">Rates below are Anthropic list prices. B2C accounts start 60% below them and can progress to 80% off; B2B rates are negotiated.</T></div><div className="table-scroll"><table className="mtable"><thead><tr><T k="th_model" as="th">Model</T><T k="th_ctx" as="th">Context</T><T k="th_in" as="th">Input / 1M</T><T k="th_out" as="th">Output / 1M</T><T k="th_best" as="th">Best for</T></tr></thead><tbody>{modelRows.map(([name,id,ctx,input,output,best], index) => <tr key={id}><td><span className="mname">{name}</span>{index === 0 && <T k="latest_badge" as="span" className="model-badge">Latest</T>}<br /><code>{id}</code></td><td><span className={`context-badge ${ctx === "1M" ? "context-full" : ""}`}>{ctx}</span></td><td className="mprice">{input}</td><td className="mprice">{output}</td><T k={best} as="td">Use case</T></tr>)}</tbody></table></div><T k="models_price_note" as="p" className="mnote">Official rates shown per million tokens; your active account discount is applied when balance is charged.</T><PageActions /></div></section></MarketingFrame>;
}

export function DocsPage() {
  return <MarketingFrame><PageHero eyebrow="nav_docs" title="docs_h" subtitle="docs_sub" /><section className="borderless"><div className="wrap">
    <div className="doc-block"><T k="docs_base" as="h3">Base URL</T><pre className="codebox">https://api.apitoken.sale            <span className="c"># Anthropic Messages API</span></pre></div>
    <div className="doc-block"><T k="docs_qs" as="h3">Quickstart</T><div className="doc-grid"><div><T k="docs_anthropic" as="p" className="code-label">Anthropic Messages API</T><pre className="codebox"><span className="k">curl</span> https://api.apitoken.sale/v1/messages {`\\\n`}  -H <span className="g">&quot;x-api-key: $APITOKEN_API_KEY&quot;</span> {`\\\n`}  -H <span className="g">&quot;anthropic-version: 2023-06-01&quot;</span> {`\\\n`}  -H <span className="g">&quot;content-type: application/json&quot;</span> {`\\\n`}  -d <span className="g">&apos;{`{"model":"claude-opus-4-8",`}{`\n    "max_tokens":1024,`}{`\n    "messages":[{"role":"user",`}{`\n    "content":"Hello"}]}'`}</span></pre></div><div><T k="docs_auth" as="p" className="code-label">Authentication</T><div className="codebox"><span className="c"># The raw key is shown once when created</span>{`\n`}x-api-key: <span className="g">sk-pool-•••</span>{`\n`}anthropic-version: <span className="g">2023-06-01</span></div></div></div></div>
    <div className="doc-block"><T k="docs_auth" as="h3">Authentication</T><T k="docs_auth_p" as="p" className="docs-copy">Create a key in your dashboard and pass it as x-api-key.</T><PageActions primaryOnly /></div>
  </div></section></MarketingFrame>;
}

const integrations = [
  ["claude-code","Claude Code","int_cc_tag"], ["cursor","Cursor","int_cur_tag"], ["cline","Cline","int_cli_tag"],
  ["continue","Continue","int_con_tag"], ["zed","Zed","int_zed_tag"], ["sdk","SDK (Python / TS)","int_sdk_tag"],
] as const;

export function IntegrationsPage() {
  return <MarketingFrame><PageHero eyebrow="nav_int" title="int_h" subtitle="int_sub" /><section className="borderless"><div className="wrap"><div className="steps" data-reveal-stagger>{integrations.map(([slug,name,tag], index) => <Link className="step" href={`/int-${slug}`} key={slug}><div className="n">{String(index + 1).padStart(2,"0")}</div><h3>{name}</h3><T k={tag} as="p">Integration description</T><T k="int_open" as="span" className="step-cta">Open guide</T></Link>)}</div></div></section></MarketingFrame>;
}

const guideCode: Record<string, ReactNode> = {
  "claude-code": <><span className="c"># set once in your shell profile</span>{`\n`}<span className="k">export</span> ANTHROPIC_BASE_URL=https://api.apitoken.sale{`\n`}<span className="k">export</span> ANTHROPIC_API_KEY=sk-pool-•••{`\n\n`}<span className="c"># then just run</span>{`\n`}<span className="k">claude</span></>,
  cursor: <><span className="c"># Cursor → Settings → Models → Anthropic API</span>{`\n`}Base URL : https://api.apitoken.sale{`\n`}API key  : sk-pool-•••{`\n`}Model    : claude-opus-4-8</>,
  cline: <><span className="c"># Cline → Settings</span>{`\n`}API Provider : Anthropic{`\n`}Base URL     : https://api.apitoken.sale{`\n`}API Key      : sk-pool-•••{`\n`}Model        : claude-opus-4-8</>,
  continue: <><span className="c">{"// ~/.continue/config.json"}</span>{`\n`}{`{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "https://api.apitoken.sale",\n    "apiKey": "sk-pool-•••",\n    "model": "claude-opus-4-8"\n  }]\n}`}</>,
  zed: <><span className="c">{"// Zed settings.json"}</span>{`\n`}{`{\n  "assistant": {\n    "default_model": {\n      "provider": "anthropic",\n      "model": "claude-opus-4-8"\n    }\n  },\n  "language_models": {\n    "anthropic": { "api_url": "https://api.apitoken.sale" }\n  }\n}`}</>,
  sdk: <><span className="c"># Python</span>{`\n`}<span className="k">from</span> anthropic <span className="k">import</span> Anthropic{`\n`}client = Anthropic({`\n`}    base_url=<span className="g">&quot;https://api.apitoken.sale&quot;</span>,{`\n`}    api_key=<span className="g">&quot;sk-pool-•••&quot;</span>,{`\n`}){`\n`}msg = client.messages.create({`\n`}    model=<span className="g">&quot;claude-opus-4-8&quot;</span>,{`\n`}    max_tokens=1024,{`\n`}    messages=[{`{"role":"user","content":"Hello"}`}],{`\n`})</>,
};

export function IntegrationGuidePage({ slug }: { slug: string }) {
  const found = integrations.find(([candidate]) => candidate === slug);
  if (!found) return null;
  const [,name,tag] = found;
  return <MarketingFrame><div className="page-hero"><div className="wrap"><Link className="auth-back" href="/integrations"><T k="int_back">← All integrations</T></Link><T k="nav_int" as="span" className="eyebrow">Integrations</T><h1>{name}</h1><T k={tag} as="p">Integration description</T></div></div><section className="borderless"><div className="wrap"><div className="steps guide-steps" data-reveal-stagger>{[["int_s1_h","int_s1_p"],["int_s2_h","int_s2_p"],["int_s3_h","int_s3_p"]].map(([title,text], index) => <div className="step" key={title}><div className="n">{String(index + 1).padStart(2,"0")}</div><T k={title} as="h3">Step</T><T k={text} as="p">Description</T></div>)}</div><div className="doc-block"><T k="int_cfg" as="h3">Configuration</T><pre className="codebox">{guideCode[slug]}</pre><PageActions /></div></div></section></MarketingFrame>;
}

const legal = {
  terms: { title: "legal_terms_h", paragraphs: ["terms_p0","terms_p1","terms_p2","terms_p3","terms_p4"] },
  privacy: { title: "legal_privacy_h", paragraphs: ["privacy_p0","privacy_p1","privacy_p2","privacy_p3","privacy_p4"] },
} as const;

export function LegalPage({ kind }: { kind: keyof typeof legal }) {
  const content = legal[kind];
  return <MarketingFrame><div className="page-hero"><div className="wrap"><T k="nav_legal" as="span" className="eyebrow">Legal</T><T k={content.title} as="h1">Legal</T><T k="legal_updated" as="p">Last updated: July 2026</T></div></div><section className="borderless"><div className="wrap legal-wrap"><T k="legal_draft" as="p" className="legal-note">Draft template.</T>{content.paragraphs.map((key) => <T k={key} as="p" key={key}>Legal paragraph</T>)}</div></section></MarketingFrame>;
}

function PageActions({ primaryOnly = false }: { primaryOnly?: boolean }) {
  return <div className="hero-cta page-actions"><Link className="btn btn-primary" href="/register"><T k="hero_cta1">Get API key</T></Link>{!primaryOnly && <Link className="btn btn-ghost" href="/docs"><T k="hero_cta2">Read documentation</T></Link>}</div>;
}
