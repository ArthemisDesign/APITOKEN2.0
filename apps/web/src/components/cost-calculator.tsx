"use client";

import Link from "next/link";
import { useMemo, useState } from "react";

/**
 * Free Claude API cost calculator — framed around whole real tasks, not single requests.
 * Official Anthropic list rates (per 1M tokens) live here verbatim; cache rates follow
 * Anthropic's standard multipliers (read = 0.1x input, 5m write = 1.25x input).
 * Each task carries a realistic TOTAL token budget for finishing the whole job end to end.
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

// Real tasks our audience actually runs — each is a WHOLE job, with a realistic total
// token budget across every call it takes. Numbers are estimates, labelled as such.
type Task = {
  key: string;
  label: string;
  phrase: string; // used in the result line: "You pay <phrase>"
  desc: string;
  input: number;
  output: number;
  cacheR: number;
  cacheW: number;
};

const TASKS: Task[] = [
  {
    key: "month-coding",
    label: "A month of coding",
    phrase: "for a month of coding",
    desc: "Full-time development with Claude Code — all day, every day, ~22 workdays.",
    input: 3_000_000, output: 2_500_000, cacheR: 80_000_000, cacheW: 3_000_000,
  },
  {
    key: "article",
    label: "Write an article",
    phrase: "to write an article",
    desc: "Research a topic and write a polished ~1,500-word article, with two rounds of edits.",
    input: 30_000, output: 12_000, cacheR: 0, cacheW: 0,
  },
  {
    key: "game",
    label: "Build a browser game",
    phrase: "to build a game",
    desc: "An AI coding agent builds a small browser game over ~50 back-and-forth iterations.",
    input: 300_000, output: 120_000, cacheR: 3_000_000, cacheW: 400_000,
  },
  {
    key: "crypto-audit",
    label: "Audit a crypto project",
    phrase: "to audit a crypto project",
    desc: "Feed the smart contracts and docs, get a full security & tokenomics audit report.",
    input: 500_000, output: 60_000, cacheR: 5_000_000, cacheW: 500_000,
  },
  {
    key: "memecoins",
    label: "Analyze 500 memecoins",
    phrase: "to analyze 500 memecoins",
    desc: "Feed market data for 500 tokens and get one ranked, reasoned report back.",
    input: 1_200_000, output: 200_000, cacheR: 0, cacheW: 0,
  },
  {
    key: "calorie-app",
    label: "AI for a calorie app",
    phrase: "to run a calorie app for a month",
    desc: "A month of an App Store calorie tracker calling Claude to analyze real users' meals.",
    input: 20_000_000, output: 6_000_000, cacheR: 5_000_000, cacheW: 500_000,
  },
  {
    key: "support",
    label: "Support bot · 10k chats",
    phrase: "to run a support bot for 10k chats",
    desc: "A month of an AI support agent handling 10,000 customer conversations.",
    input: 8_000_000, output: 4_000_000, cacheR: 20_000_000, cacheW: 500_000,
  },
  {
    key: "summarize",
    label: "Summarize a 300-page report",
    phrase: "to summarize a 300-page report",
    desc: "Digest a long PDF end to end into a tight executive brief.",
    input: 250_000, output: 15_000, cacheR: 0, cacheW: 0,
  },
  {
    key: "team",
    label: "Company · 50+ vibe-coders",
    phrase: "for a 50-developer team, one month",
    desc: "50+ developers vibe-coding full-time for a month on one shared balance.",
    input: 150_000_000, output: 125_000_000, cacheR: 4_000_000_000, cacheW: 150_000_000,
  },
];

const DEFAULT_TASK = 0; // "A month of coding"

// Discount tiers — Starter is free, larger discounts unlock as your cumulative top-ups grow.
const TIERS = [
  { label: "Starter", discount: 60, free: true, topup: 0 },
  { label: "Builder", discount: 65, topup: 100 },
  { label: "Pro", discount: 70, topup: 250 },
  { label: "Studio", discount: 75, topup: 500 },
  { label: "Scale", discount: 80, topup: 1000 },
];

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

function taskCost(m: Model, inTok: number, outTok: number, cacheR: number, cacheW: number): number {
  return (
    (inTok / 1e6) * m.input +
    (outTok / 1e6) * m.output +
    (cacheR / 1e6) * m.input * CACHE_READ_MULT +
    (cacheW / 1e6) * m.input * CACHE_WRITE_MULT
  );
}

export function CostCalculator() {
  const [taskIdx, setTaskIdx] = useState(DEFAULT_TASK);
  const [inTok, setInTok] = useState(TASKS[DEFAULT_TASK].input);
  const [outTok, setOutTok] = useState(TASKS[DEFAULT_TASK].output);
  const [cacheR, setCacheR] = useState(TASKS[DEFAULT_TASK].cacheR);
  const [cacheW, setCacheW] = useState(TASKS[DEFAULT_TASK].cacheW);
  const [tier, setTier] = useState(0);
  const [selected, setSelected] = useState("claude-opus-4-8");
  const [advanced, setAdvanced] = useState(false);

  const task = TASKS[taskIdx];
  const discount = TIERS[tier].discount;
  const mult = 1 - discount / 100;

  function pickTask(i: number) {
    const t = TASKS[i];
    setTaskIdx(i);
    setInTok(t.input);
    setOutTok(t.output);
    setCacheR(t.cacheR);
    setCacheW(t.cacheW);
  }

  const rows = useMemo(() => {
    return MODELS.map((m) => {
      const official = taskCost(m, inTok, outTok, cacheR, cacheW);
      const yours = official * mult;
      return { m, official, yours, save: official - yours };
    });
  }, [inTok, outTok, cacheR, cacheW, mult]);

  const cheapestId = useMemo(() => rows.reduce((a, b) => (b.yours < a.yours ? b : a)).m.id, [rows]);

  const hero = rows.find((r) => r.m.id === selected) ?? rows[0];
  const scaleMult = 100 / (100 - discount);

  return (
    <div className="calc">
      <div className="calc-grid">
        {/* ---------- Inputs ---------- */}
        <div className="calc-panel">
          <div className="calc-field">
            <label>
              What do you want to do? <span>pick a whole task, not one request</span>
            </label>
            <div className="calc-tasks">
              {TASKS.map((t, i) => (
                <button key={t.key} type="button" className={`calc-task ${taskIdx === i ? "on" : ""}`} onClick={() => pickTask(i)}>
                  {t.label}
                </button>
              ))}
            </div>
          </div>

          <p className="calc-task-desc">{task.desc}</p>

          <button type="button" className="calc-advanced-toggle" onClick={() => setAdvanced((v) => !v)}>
            <span className="calc-plus">{advanced ? "−" : "+"}</span> Adjust the tokens yourself
          </button>
          {advanced && (
            <div className="calc-advanced">
              <div className="calc-field">
                <label htmlFor="calc-in">
                  Input tokens <span>everything you send</span>
                </label>
                <input id="calc-in" inputMode="numeric" value={fmt(inTok)} onChange={(e) => setInTok(parseNum(e.target.value))} />
              </div>
              <div className="calc-field">
                <label htmlFor="calc-out">
                  Output tokens <span>everything Claude writes</span>
                </label>
                <input id="calc-out" inputMode="numeric" value={fmt(outTok)} onChange={(e) => setOutTok(parseNum(e.target.value))} />
              </div>
              <div className="calc-field">
                <label htmlFor="calc-cr">
                  Cache read tokens <span>0.1× input rate</span>
                </label>
                <input id="calc-cr" inputMode="numeric" value={fmt(cacheR)} onChange={(e) => setCacheR(parseNum(e.target.value))} />
              </div>
              <div className="calc-field" style={{ marginBottom: 0 }}>
                <label htmlFor="calc-cw">
                  Cache write tokens <span>1.25× input rate</span>
                </label>
                <input id="calc-cw" inputMode="numeric" value={fmt(cacheW)} onChange={(e) => setCacheW(parseNum(e.target.value))} />
              </div>
            </div>
          )}

          <div className="calc-divider" />

          <div className="calc-field" style={{ marginBottom: 0 }}>
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
            <Link className="calc-tier-cta" href="/#pricing">
              <span>
                {TIERS[tier].free
                  ? "You’re on Starter — −60%, free. See how bigger discounts unlock"
                  : `Top up $${TIERS[tier].topup.toLocaleString("en-US")} total to unlock −${discount}%`}
              </span>
              <span className="calc-tier-cta-arrow" aria-hidden="true">→</span>
            </Link>
          </div>
        </div>

        {/* ---------- Result ---------- */}
        <div className="calc-result">
          <div className="calc-result-head">
            <span className="tag">You pay {task.phrase}</span>
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
          <p className="calc-sub">
            {task.phrase} on {hero.m.name} · official Anthropic price minus your {discount}% discount
          </p>

          <div className="calc-save">
            <span>You save</span>
            <b>{usd(hero.save)}</b>
            <em>≈ ×{scaleMult.toLocaleString("en-US", { maximumFractionDigits: 2 })} more work per $</em>
          </div>

          <Link className="btn btn-primary calc-cta" href="/register">
            Start free — $10 at official prices
          </Link>
          <p className="calc-note">Same Anthropic Messages API, same models, same responses. You just pay less per call.</p>
        </div>
      </div>

      {/* ---------- Full comparison ---------- */}
      <div className="calc-table-head">
        <h2>Every Claude model for this task</h2>
        <p>
          Official Anthropic price vs your apiToken.sale price at −{discount}%, {task.phrase}.
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
        Each task uses a realistic total number of tokens to finish the whole job — expand &ldquo;Adjust the tokens
        yourself&rdquo; to tune it. Claude Sonnet 5 shows its introductory official rate ($2 / $10 per 1M) in effect through
        2026-08-31; it returns to $3 / $15 on 2026-09-01. Cache read is billed at 0.1× the input rate and 5-minute cache writes
        at 1.25×, per Anthropic&rsquo;s standard pricing. Estimates only — your real bill depends on exact token usage.
      </p>
    </div>
  );
}
