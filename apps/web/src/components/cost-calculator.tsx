"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { claudeModels, openaiModels } from "@/lib/models";
import { B2C_DISCOUNT_PERCENT } from "@/lib/pricing-tiers";
import { LocalizedLink } from "./translated";

/**
 * Free Claude/GPT API cost calculator — framed around whole real tasks, not single requests.
 * Official list and cache rates come from the shared model catalog.
 * Each task carries a realistic TOTAL token budget for finishing the whole job end to end.
 * The apiToken.sale price is simply official x (1 - discount). Fully client-side.
 */

type Provider = "anthropic" | "openai";

type Model = {
  name: string;
  id: string;
  context: string;
  input: number; // $ / 1M input tokens
  output: number; // $ / 1M output tokens
  cacheRead: number; // $ / 1M cached input tokens
  cacheWrite: number; // $ / 1M cache-write tokens
  note?: string;
};

const CLAUDE_MODELS: Model[] = claudeModels.map((model) => {
  const hasIntroRate = model.id === "claude-sonnet-5";
  return {
    name: model.name,
    id: model.id,
    context: model.context,
    input: hasIntroRate ? 2 : model.inputPerM,
    output: hasIntroRate ? 10 : model.outputPerM,
    cacheRead: hasIntroRate ? 0.2 : model.cacheReadPerM,
    cacheWrite: hasIntroRate ? 2.5 : model.cacheWrite5mPerM,
    note: hasIntroRate ? "intro rate" : undefined,
  };
});

// The calculator estimates text token spend; image models meter per image-output token instead.
const GPT_MODELS: Model[] = openaiModels
  .filter((model) => model.tier !== "Image")
  .map((model) => ({
    name: model.name,
    id: model.id,
    context: model.context,
    input: model.inputPerM,
    output: model.outputPerM,
    cacheRead: model.cachedInputPerM,
    cacheWrite: model.cacheWritePerM,
  }));

const PROVIDERS: Record<Provider, {
  label: string;
  models: Model[];
  officialName: string;
  apiDescription: string;
}> = {
  anthropic: {
    label: "Claude",
    models: CLAUDE_MODELS,
    officialName: "Anthropic",
    apiDescription: "Anthropic Messages API",
  },
  openai: {
    label: "GPT",
    models: GPT_MODELS,
    officialName: "OpenAI",
    apiDescription: "OpenAI-compatible API",
  },
};

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
    desc: "Full-time development with an AI coding agent — all day, every day, ~22 workdays.",
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
    desc: "A month of an App Store calorie tracker calling an AI model to analyze real users' meals.",
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

// Flat B2C pricing: one 50% discount on every request — no tiers to pick.
const DISCOUNT = B2C_DISCOUNT_PERCENT;
const MULT = 1 - DISCOUNT / 100;

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
    (cacheR / 1e6) * m.cacheRead +
    (cacheW / 1e6) * m.cacheWrite
  );
}

export function CostCalculator() {
  const [provider, setProvider] = useState<Provider>("anthropic");
  const [taskIdx, setTaskIdx] = useState(DEFAULT_TASK);
  const [inTok, setInTok] = useState(TASKS[DEFAULT_TASK].input);
  const [outTok, setOutTok] = useState(TASKS[DEFAULT_TASK].output);
  const [cacheR, setCacheR] = useState(TASKS[DEFAULT_TASK].cacheR);
  const [cacheW, setCacheW] = useState(TASKS[DEFAULT_TASK].cacheW);
  const [selected, setSelected] = useState(CLAUDE_MODELS[0].id);
  const [advanced, setAdvanced] = useState(false);

  const providerInfo = PROVIDERS[provider];
  const models = providerInfo.models;
  const task = TASKS[taskIdx];
  const discount = DISCOUNT;
  const mult = MULT;

  function pickTask(i: number) {
    const t = TASKS[i];
    setTaskIdx(i);
    setInTok(t.input);
    setOutTok(t.output);
    setCacheR(t.cacheR);
    setCacheW(t.cacheW);
  }

  function pickProvider(nextProvider: Provider) {
    setProvider(nextProvider);
    setSelected(PROVIDERS[nextProvider].models[0].id);
  }

  const rows = useMemo(() => {
    return models.map((m) => {
      const official = taskCost(m, inTok, outTok, cacheR, cacheW);
      const yours = official * mult;
      return { m, official, yours, save: official - yours };
    });
  }, [models, inTok, outTok, cacheR, cacheW, mult]);

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
              Model provider <span>compare either model family with the same task</span>
            </label>
            <div className="calc-providers" role="group" aria-label="Model provider">
              {(Object.keys(PROVIDERS) as Provider[]).map((key) => (
                <button
                  key={key}
                  type="button"
                  className={`calc-provider ${provider === key ? "on" : ""}`}
                  aria-pressed={provider === key}
                  onClick={() => pickProvider(key)}
                >
                  {PROVIDERS[key].label} models
                </button>
              ))}
            </div>
          </div>

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
                  Output tokens <span>everything the model writes</span>
                </label>
                <input id="calc-out" inputMode="numeric" value={fmt(outTok)} onChange={(e) => setOutTok(parseNum(e.target.value))} />
              </div>
              <div className="calc-field">
                <label htmlFor="calc-cr">
                  Cache read tokens <span>discounted cached input</span>
                </label>
                <input id="calc-cr" inputMode="numeric" value={fmt(cacheR)} onChange={(e) => setCacheR(parseNum(e.target.value))} />
              </div>
              <div className="calc-field" style={{ marginBottom: 0 }}>
                <label htmlFor="calc-cw">
                  Cache write tokens <span>provider-specific write rate</span>
                </label>
                <input id="calc-cw" inputMode="numeric" value={fmt(cacheW)} onChange={(e) => setCacheW(parseNum(e.target.value))} />
              </div>
            </div>
          )}

          <div className="calc-divider" />

          <div className="calc-field" style={{ marginBottom: 0 }}>
            <label>
              Your discount <span>one flat rate — no tiers, no minimums</span>
            </label>
            <Link className="calc-tier-cta" href="/#pricing">
              <span>Flat −{DISCOUNT}% off official prices on every request — see how billing works</span>
              <span className="calc-tier-cta-arrow" aria-hidden="true">→</span>
            </Link>
          </div>
        </div>

        {/* ---------- Result ---------- */}
        <div className="calc-result">
          <div className="calc-result-head">
            <span className="tag">You pay {task.phrase}</span>
            <div className="calc-model-chips">
              {models.map((m) => (
                <button key={m.id} type="button" className={`calc-mchip ${selected === m.id ? "on" : ""}`} onClick={() => setSelected(m.id)}>
                  {m.name.replace(provider === "anthropic" ? "Claude " : "GPT-", "")}
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
            {task.phrase} on {hero.m.name} · official {providerInfo.officialName} price minus your {discount}% discount
          </p>

          <div className="calc-save">
            <span>You save</span>
            <b>{usd(hero.save)}</b>
            <em>≈ ×{scaleMult.toLocaleString("en-US", { maximumFractionDigits: 2 })} more work per $</em>
          </div>

          <LocalizedLink className="btn btn-primary calc-cta" href="/register">
            Start free — $5 platform bonus
          </LocalizedLink>
          <p className="calc-note">Same {providerInfo.apiDescription}, models and responses. You just pay less per call.</p>
        </div>
      </div>

      {/* ---------- Full comparison ---------- */}
      <div className="calc-table-head">
        <h2>Every {providerInfo.label} model for this task</h2>
        <p>
          Official {providerInfo.officialName} price vs your apiToken.sale price at −{discount}%, {task.phrase}.
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
        yourself&rdquo; to tune it. {provider === "anthropic"
          ? "Claude Sonnet 5 shows its introductory $2 / $10 rate through 2026-08-31; cache reads use 0.1× input and 5-minute cache writes use 1.25× input."
          : "GPT rows use standard rates from the pinned catalog. GPT-5.6 cache writes use 1.25× input; GPT-5.5/5.4 use 1×. A single request over 272K input tokens has a long-context premium not inferred from these whole-task totals."}{" "}
        Estimates only — your real bill depends on exact token usage.
      </p>
    </div>
  );
}
