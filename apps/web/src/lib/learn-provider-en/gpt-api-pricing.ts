import type { LearnArticle } from "../learn";

export const article: LearnArticle = {
    slug: "gpt-api-pricing",
    cluster: "explain",
    title: "GPT API Pricing Explained",
    h1: "GPT API pricing: input, cache, output and long context",
    description: "Understand GPT API pricing for GPT-5.6 Sol, Terra and Luna: input, cached input, cache write, output, long-context rates and the flat 50% apiToken.sale discount.",
    keywords: ["gpt api pricing", "gpt-5.6 price", "gpt api cost", "gpt token pricing", "gpt-5.6 sol price", "cheapest gpt api"],
    dek: "GPT cost is a sum of exact token legs, not a price per request. The model tier, cached tokens and input length determine official spend; apiToken.sale then removes 50% from that spend.",
    sections: [
      { h2: "Current GPT-5.6 rates", blocks: [
        { type: "table", headers: ["Model", "Official input / cached / output", "Price here after 50%"], rows: [
          ["gpt-5.6-sol", "$5 / $0.50 / $30", "$2.50 / $0.25 / $15"],
          ["gpt-5.6-terra", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gpt-5.6-luna", "$0.20 / $0.02 / $1.20", "$0.10 / $0.01 / $0.60"],
        ] },
        { type: "p", text: "Rates are per 1M tokens. gpt-5.6 is an alias of gpt-5.6-sol, so it has the same price rather than a separate tariff." },
      ] },
      { h2: "Cache write and long-context rules", blocks: [
        { type: "list", items: [
          "GPT-5.6 cache writes bill at 125% of normal input; cached reads bill at 10% of input.",
          "Above 272K input tokens, the whole request uses 2× input and 1.5× output rates.",
          "Reasoning tokens appear in output usage and are not charged a second time as a separate leg.",
          "The dashboard records the settled token usage and exact discounted charge for each request.",
        ] },
        { type: "note", text: "A cheaper model often saves more than prompt trimming: Terra costs 40% of Sol per token, while Luna costs 4% of Sol. Route by task difficulty instead of using the flagship everywhere." },
      ] },
    ],
    faq: [
      { q: "How much does GPT-5.6 cost per 1M tokens?", a: "Officially Sol is $5 input and $30 output, Terra $2/$12, and Luna $0.20/$1.20. apiToken.sale applies a flat 50% discount to those exact legs." },
      { q: "What counts as cached input?", a: "Repeated prompt prefixes that the provider serves from cache. The terminal usage determines the cached leg; you are not charged both cached and fresh input for the same token." },
      { q: "When does long-context pricing start?", a: "When input exceeds 272K tokens. The whole request then bills at 2× input and 1.5× output before the 50% discount." },
    ],
    related: ["gpt-5-6-sol-vs-terra-vs-luna", "how-to-buy-gpt-api-key", "openai-api-quickstart", "save-tokens-on-claude-api"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
