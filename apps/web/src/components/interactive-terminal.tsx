import { T } from "./translated";

export function InteractiveTerminal() {
  return <div className="term-stage">
    <div className="term">
      <div className="term-bar">
        <span className="term-controls" aria-hidden="true"><i className="term-close" /><i className="term-minimize" /><i className="term-zoom" /></span>
        <T k="term_hint" className="term-title">one key · all models</T>
      </div>
      <div className="term-body"><span className="c"># set one key, call any Claude model</span><br /><span className="k">export</span> <span className="w">APITOKEN_API_KEY=sk-pool-•••</span><br /><br /><span className="k">curl</span> https://api.apitoken.sale/v1/messages \<br />&nbsp;&nbsp;-H <span className="g">&quot;x-api-key: $APITOKEN_API_KEY&quot;</span> \<br />&nbsp;&nbsp;-d <span className="g">&apos;{`{"model":"claude-opus-4-8",`}</span><br />&nbsp;&nbsp;&nbsp;&nbsp;<span className="g">{`"messages":[{"role":"user",`}</span><br />&nbsp;&nbsp;&nbsp;&nbsp;<span className="g">{`"content":"ship it"}]}'`}</span><br /><br /><span className="c">→ 200 OK · streamed · official API spend metered</span></div>
    </div>
  </div>;
}
