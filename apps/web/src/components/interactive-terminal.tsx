"use client";

import type { PointerEvent } from "react";
import { T } from "./translated";

export function InteractiveTerminal() {
  function move(event: PointerEvent<HTMLDivElement>) {
    if (window.matchMedia("(prefers-reduced-motion: reduce), (pointer: coarse)").matches) return;
    const card = event.currentTarget;
    const bounds = card.getBoundingClientRect();
    const x = (event.clientX - bounds.left) / bounds.width - .5;
    const y = (event.clientY - bounds.top) / bounds.height - .5;
    card.style.setProperty("--term-x", `${(x * 5).toFixed(2)}px`);
    card.style.setProperty("--term-y", `${(y * 4).toFixed(2)}px`);
    card.style.setProperty("--term-rx", `${(-y * 1.6).toFixed(2)}deg`);
    card.style.setProperty("--term-ry", `${(x * 2).toFixed(2)}deg`);
  }

  function reset(event: PointerEvent<HTMLDivElement>) {
    const card = event.currentTarget;
    card.style.setProperty("--term-x", "0px");
    card.style.setProperty("--term-y", "0px");
    card.style.setProperty("--term-rx", "0deg");
    card.style.setProperty("--term-ry", "0deg");
  }

  return <div className="term-stage">
    <div className="term" onPointerMove={move} onPointerLeave={reset}>
      <div className="term-bar">
        <span className="term-controls" aria-hidden="true"><i className="term-close" /><i className="term-minimize" /><i className="term-zoom" /></span>
        <T k="term_hint" className="term-title">one key · all models</T>
      </div>
      <div className="term-body"><span className="c"># set one key, call any Claude model</span><br /><span className="k">export</span> <span className="w">APITOKEN_API_KEY=sk-pool-•••</span><br /><br /><span className="k">curl</span> https://api.apitoken.sale/v1/messages \<br />&nbsp;&nbsp;-H <span className="g">&quot;x-api-key: $APITOKEN_API_KEY&quot;</span> \<br />&nbsp;&nbsp;-d <span className="g">&apos;{`{"model":"claude-opus-4-8",`}</span><br />&nbsp;&nbsp;&nbsp;&nbsp;<span className="g">{`"messages":[{"role":"user",`}</span><br />&nbsp;&nbsp;&nbsp;&nbsp;<span className="g">{`"content":"ship it"}]}'`}</span><br /><br /><span className="c">→ 200 OK · streamed · billed by token</span></div>
    </div>
  </div>;
}
