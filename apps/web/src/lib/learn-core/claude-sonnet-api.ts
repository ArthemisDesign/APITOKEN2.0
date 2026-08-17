import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-sonnet-api",
  cluster: "free",
  title: "Claude Sonnet API Access",
  h1: "Claude Sonnet through the API",
  description: "Use Claude Sonnet 5 and Sonnet 4.6 through apiToken.sale — the default model for daily coding and agents, at a flat 50% off official API pricing.",
  keywords: ["claude sonnet api", "claude sonnet 5 api", "sonnet api key", "claude sonnet pricing", "best claude model for coding", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
  dek: "Sonnet is the workhorse: fast enough for interactive coding, smart enough for real agent workflows. apiToken.sale serves Sonnet 5 and Sonnet 4.6 on one discounted balance.",
  sections: [
    { h2: "The daily-driver model", blocks: [
      { type: "p", text: "For most coding and agent tasks, Sonnet is the right default — a strong balance of quality, speed and cost. Reserve Opus for the genuinely hard problems." },
    ] },
    { h2: "Sonnet pricing note", blocks: [
      { type: "p", text: "Claude Sonnet 5 (claude-sonnet-5) ships with introductory official rates, and the engine always applies the current effective rate before your discount. Sonnet 4.6 remains available on the same key." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
        ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
      ] },
      { type: "link", text: "Claude Sonnet 5 pricing in detail (cache rates, context, FAQ)", href: "/models/claude-sonnet-5" },
      cta(),
    ] },
  ],
  faq: [
    { q: "Which Sonnet models can I use?", a: "Claude Sonnet 5 (claude-sonnet-5) and Claude Sonnet 4.6, on the same balance as Opus and Haiku." },
    { q: "Is Sonnet good for coding?", a: "Yes — Sonnet is the recommended default for everyday coding and agent workflows." },
  ],
  related: ["claude-opus-vs-sonnet", "claude-opus-api", "claude-haiku-api", "claude-api-key-for-cursor"],
  updated: "2026-07-17",
};
