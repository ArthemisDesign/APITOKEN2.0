import type { LearnArticle } from "../learn";

export const article: LearnArticle = {
    slug: "gemini-api-pricing",
    cluster: "explain",
    title: "Gemini API Pricing Explained",
    h1: "Gemini API pricing: Pro, Flash, Flash-Lite and image output",
    description: "Compare Gemini API token prices for Pro, Flash, Flash-Lite and Nano Banana 2, including cached input, long context, image output and apiToken.sale's flat 50% discount.",
    keywords: ["gemini api pricing", "gemini api cost", "gemini token price", "gemini flash price", "gemini pro price", "cheap gemini api"],
    dek: "Gemini pricing depends on model tier, cached input, output modality and — for Pro — context length. The gateway settles those exact official legs, then applies a flat 50% discount.",
    sections: [
      { h2: "Representative text-model rates", blocks: [
        { type: "table", headers: ["Model", "Official input / cached / output", "Price here after 50%"], rows: [
          ["gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gemini-3.6-flash", "$1.50 / $0.15 / $7.50", "$0.75 / $0.075 / $3.75"],
          ["gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
          ["gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
        ] },
        { type: "p", text: "All figures are per 1M tokens. Cached input is an independent usage leg reported by the provider; it is not added on top of fresh input for the same tokens." },
      ] },
      { h2: "Long context and images", blocks: [
        { type: "list", items: [
          "Gemini 3.1 Pro Preview requests above 200K input tokens use $4 input and $18 output per 1M on the whole request.",
          "Gemini 3.1 Flash Image charges text output at $3 and image output at $60 per 1M image tokens.",
          "Flash Image cached input bills at the full input rate; it does not receive the text-model cache discount.",
          "The 50% B2C discount applies after the exact official legs are calculated.",
        ] },
      ] },
    ],
    faq: [
      { q: "What is the cheapest Gemini model?", a: "Among the published text tiers, Gemini 2.5 Flash-Lite is $0.10 input and $0.40 output per 1M official, or $0.05/$0.20 after the flat 50% discount." },
      { q: "When does Gemini long-context pricing apply?", a: "For Gemini 3.1 Pro Preview above 200K input tokens. The whole request then uses the higher input, cached-input and output rates." },
      { q: "How is Gemini image output priced?", a: "Gemini 3.1 Flash Image bills rendered output at $60 per 1M image-output tokens officially, or $30 after the flat 50% discount." },
    ],
    related: ["gemini-pro-vs-flash-vs-flash-lite", "how-to-buy-gemini-api-key", "nano-banana-2-api-guide", "how-billing-works"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
