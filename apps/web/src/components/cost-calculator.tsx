"use client";

import Link from "next/link";
import { useMemo, useState } from "react";

/**
 * Free Claude API cost calculator.
 * Official Anthropic list rates (per 1M tokens) live here verbatim; cache rates
 * follow Anthropic's standard multipliers (read = 0.1x input, 5m write = 1.25x input).
 * The apiToken.sale price is simply official x (1 - discount). Fully client-side.
 */

type Model = {
  name: string;
  id: string;
  context: string;
  input: number; // $ / 1M input tokens
  output: number; // $ / 1M output tokens
  note?: string;
};

const MODELS: Model[] = [
  { name: "Claude Opus 4.8", id: "claude-opus-4-8", context: "1M", input: 5, output: 25 },
  { name: "Claude Opus 4.7", id: "claude-opus-4-7", context: "1M", input: 5, output: 25 },
  { name: "Claude Sonnet 5", id: "claude-sonnet-5", context: "1M", input: 2, output: 10, note: "intro rate" },
  { name: "Claude Sonnet 4.6", id: "claude-sonnet-4-6", context: "1M", input: 3, output: 15 },
  { name: "Claude Haiku 4.5", id: "claude-haiku-4-5", context: "200K", input: 1, output: 5 },
];

const CACHE_READ_MULT = 0.1; // Anthropic: cache read = 0.1x input
const CACHE_WRITE_MULT = 1.25; // Anthropic: 5-minute cache write = 1.25x input

// Discount tiers — Starter is free, larger discounts unlock as you top up more.
const TIERS = [
  { label: "Starter", discount: 60, free: true },
  { label: "Builder", discount: 65 },
  { label: "Pro", discount: 70 },
  { label: "Studio", discount: 75 },
  { label: "Scale", discount: 80 },
];

const PRESETS = [
  { label: "Short Q&A", input: 500, output: 300 },
  { label: "Chatbot turn", input: 2_000, output: 500 },
  { label: "Long document", input: 50_000, output: 2_000 },
  { label: "Coding agent", input: 15_000, output: 4_000 },
];

const REQ_PRESETS = [1, 1_000, 100_000, 1_000_000];

function usd(v: number): string {
  if (!isFinite(v) || v <= 0) return "$0.00";
  const d = v < 0.01 ? 5 : v < 1 ? 4 : 2;
  return "$" + v.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: d });
}

function fmt(n: number): string {
  return n.toLocaleString("en-US");
}

function parseNum(s: string): number {
  const n = parseInt(s.replace(/\D/g, ""), 10);
  return isFinite(n) ? n : 0;
}

function modelCost(m: Model, inTok: number, outTok: number, cacheR: number, cacheW: number): number {
  return (
    (inTok / 1e6) * m.input +
    (outTok / 1e6) * m.output +
    (cacheR / 1e6) * m.input * CACHE_READ_MULT +
    (cacheW / 1e6) * m.input * CACHE_WRITE_MULT
  );
}

export function CostCalculator() {
  const [inTok, setInTok] = useState(15_000);
  const [outTok, setOutTok] = useState(4_000);
  const [cacheR, setCacheR] = useState(0);
  const [cacheW, setCacheW] = useState(0);
  const [reqs, setReqs] = useState(1_000);
  const [tier, setTier] = useState(0);
  const [selected, setSelected] = useState("claude-opus-4-8");
  const [advanced, setAdvanced] = useState(false);

  const discount = TIERS[tier].discount;
  const mult = 1 - discount / 100;

  const rows = useMemo(() => {
    return MODELS.map((m) => {
      const per = modelCost(m, inTok, outTok, cacheR, cacheW);
      const official = per * reqs;
      const yours = official * mult;
      return { m, per, official, yours, save: official - yours };
    });
  }, [inTok, outTok, cacheR, cacheW, reqs, mult]);

  const cheapestId = useMemo(() => {
    return rows.reduce((a, b) => (b.yours < a.yours ? b : a)).m.id;
  }, [rows]);

  const hero = rows.find((r) => r.m.id === selected) ?? rows[0];
  const totalSave = rows.find((r) => r.m.id === selected)?.save ?? 0;
  const scaleMult = 100 / (100 - discount);

  const reqLabel = reqs === 1 ? "1 request" : `${fmt(reqs)} requests / month`;

  return (
    <div className="calc">
      <div className="calc-grid">
        {/* ---------- Inputs ---------- */}
        <div className="calc-panel">
          <div className="calc-field">
            <label htmlFor="calc-in">
              Input tokens <span>the prompt you send</span>
            </label>
            <input
              id="calc-in"
              inputMode="numeric"
              value={fmt(inTok)}
              onChange={(e) => setInTok(parseNum(e.target.value))}
              aria-label="Input tokens per request"
            />
          </div>

          <div className="calc-field">
            <label htmlFor="calc-out">
              Output tokens <span>what Claude writes back</span>
            </label>
            <input
              id="calc-out"
              inputMode="numeric"
              value={fmt(outTok)}
              onChange={(e) => setOutTok(parseNum(e.target.value))}
              aria-label="Output tokens per request"
            />
          </div>

          <div className="calc-presets">
            {PRESETS.map((p) => (
              <button
                key={p.label}
                type="button"
                className="calc-chip"
                onClick={() => {
                  setInTok(p.input);
                  setOutTok(p.output);
                }}
              >
                {p.label}
              </button>
            ))}
          </div>

          <button type="button" className="calc-advanced-toggle" onClick={() => setAdvanced((v) => !v)}>
            <span className="calc-plus">{advanced ? "−" : "+"}</span> Prompt caching (optional)
          </button>
          {advanced && (
            <div className="calc-advanced">
              <div className="calc-field">
                <label htmlFor="calc-cr">
                  Cache read tokens <span>0.1× input rate</span>
                </label>
                <input id="calc-cr" inputMode="numeric" value={fmt(cacheR)} onChange={(e) => setCacheR(parseNum(e.target.value))} />
              </div>
              <div className="calc-field">
                <label htmlFor="calc-cw">
                  Cache write tokens <span>1.25× input rate</span>
                </label>
                <input id="calc-cw" inputMode="numeric" value={fmt(cacheW)} onChange={(e) => setCacheW(parseNum(e.target.value))} />
              </div>
            </div>
          )}

          <div className="calc-divider" />

          <div className="calc-field">
            <label htmlFor="calc-req">
              How many requests? <span>scales every price below</span>
            </label>
            <input id="calc-req" inputMode="numeric" value={fmt(reqs)} onChange={(e) => setReqs(Math.max(1, parseNum(e.target.value)))} />
          </div>
          <div className="calc-presets">
            {REQ_PRESETS.map((r) => (
              <button key={r} type="button" className={`calc-chip ${reqs === r ? "on" : ""}`} onClick={() => setReqs(r)}>
                {r === 1 ? "1" : fmt(r)}
              </button>
            ))}
          </div>

          <div className="calc-divider" />

          <div className="calc-field">
            <label>
              Your discount <span>Starter is free — bigger tiers unlock as you top up</span>
            </label>
            <div className="calc-tiers">
              {TIERS.map((t, i) => (
                <button key={t.label} type="button" className={`calc-tier ${tier === i ? "on" : ""}`} onClick={() => setTier(i)}>
                  <b>−{t.discount}%</b>
                  <em>{t.free ? "Free" : t.label}</em>
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* ---------- Result ---------- */}
        <div className="calc-result">
          <div className="calc-result-head">
            <span className="tag">You pay for {hero.m.name}</span>
            <div className="calc-model-chips">
              {MODELS.map((m) => (
                <button key={m.id} type="button" className={`calc-mchip ${selected === m.id ? "on" : ""}`} onClick={() => setSelected(m.id)}>
                  {m.name.replace("Claude ", "")}
                </button>
              ))}
            </div>
          </div>

          <div className="calc-hero-price">
            <div className="calc-now">{usd(hero.yours)}</div>
            <div className="calc-was">
              <s>{usd(hero.official)}</s>
              <span className="tlo-badge">−{discount}%</span>
            </div>
          </div>
          <p className="calc-sub">for {reqLabel} · official Anthropic price minus your {discount}% discount</p>

          <div className="calc-save">
            <span>You save</span>
            <b>{usd(totalSave)}</b>
            <em>≈ ×{scaleMult.toLocaleString("en-US", { maximumFractionDigits: 2 })} more usage per $</em>
          </div>

          <Link className="btn btn-primary calc-cta" href="/register">
            Start free — $10 at official prices
          </Link>
          <p className="calc-note">Same Anthropic Messages API, same models, same responses. You just pay less per call.</p>
        </div>
      </div>

      {/* ---------- Full comparison ---------- */}
      <div className="calc-table-head">
        <h2>Every Claude model, side by side</h2>
        <p>
          Official Anthropic list price vs your apiToken.sale price at −{discount}%, for {reqLabel}.
        </p>
      </div>
      <div className="table-scroll">
        <table className="mtable calc-mtable">
          <thead>
            <tr>
              <th>Model</th>
              <th className="tnum">Input / 1M</th>
              <th className="tnum">Output / 1M</th>
              <th className="tnum">Official</th>
              <th className="tnum">Your price</th>
              <th className="tnum">You save</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ m, official, yours, save }) => (
              <tr key={m.id} className={m.id === cheapestId ? "calc-cheapest" : ""}>
                <td>
                  <span className="mname">{m.name}</span>
                  {m.id === cheapestId && <span className="model-badge">Cheapest</span>}
                  {m.note && <span className="model-badge">{m.note}</span>}
                  <br />
                  <code>{m.id}</code>
                </td>
                <td className="mprice tnum">${m.input}</td>
                <td className="mprice tnum">${m.output}</td>
                <td className="mprice tnum calc-official">{usd(official)}</td>
                <td className="mprice tnum calc-your">{usd(yours)}</td>
                <td className="mprice tnum calc-savecell">{usd(save)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="tier-footnote">
        Claude Sonnet 5 shows its introductory official rate ($2 / $10 per 1M) in effect through 2026-08-31; it returns to
        $3 / $15 on 2026-09-01. Cache read is billed at 0.1× the input rate and 5-minute cache writes at 1.25×, per Anthropic&rsquo;s
        standard pricing. Estimates only — your real bill depends on exact token usage.
      </p>
    </div>
  );
}
