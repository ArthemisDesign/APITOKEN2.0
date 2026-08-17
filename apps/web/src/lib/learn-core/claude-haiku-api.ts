import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-haiku-api",
  cluster: "free",
  title: "Claude Haiku API Access",
  h1: "Claude Haiku 4.5 through the API",
  description: "Access Claude Haiku 4.5 through apiToken.sale — the fastest, most economical Claude model, ideal for high-volume and low-latency tasks, at a prepaid discount.",
  keywords: ["claude haiku api", "claude haiku 4.5 api", "fastest claude model", "cheap claude model", "haiku api key", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
  dek: "Haiku is built for speed and volume: classification, extraction, routing and any task where latency and cost matter more than deep reasoning.",
  sections: [
    { h2: "When Haiku is the right call", blocks: [
      { type: "list", items: [
        "High-volume, low-latency requests.",
        "Cheap background tasks and pre-processing.",
        "Stretching your balance on work that does not need Opus.",
      ] },
      cta(),
    ] },
    { h2: "Mix models on one key", blocks: [
      { type: "p", text: "Because every model shares one key and balance, you can route cheap work to Haiku (claude-haiku-4-5) and escalate only the hard requests to Sonnet or Opus." },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (\u221250%)"], rows: [
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
      ] },
      { type: "link", text: "Claude Haiku 4.5 pricing in detail (cache rates, context, FAQ)", href: "/models/claude-haiku-4-5" },
    ] },
  ],
  faq: [
    { q: "How fast and cheap is Haiku?", a: "Haiku 4.5 is the fastest and lowest-cost Claude model, ideal for high-volume, latency-sensitive work." },
    { q: "Can I combine Haiku with other models?", a: "Yes. One key and balance covers Haiku, Sonnet and Opus, so you can route each task to the best-value model." },
  ],
  related: ["claude-sonnet-api", "claude-opus-api", "save-tokens-on-claude-api", "cheapest-claude-api"],
  updated: "2026-07-17",
};
