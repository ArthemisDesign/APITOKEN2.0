import type { LearnArticle } from "../learn";

export const article: LearnArticle = {
    slug: "kimi-api-pricing",
    cluster: "explain",
    title: "Kimi API Pricing Explained",
    h1: "Kimi API pricing: cache hits, misses, output and speed",
    description: "Understand Kimi API pricing for K3, Kimi for Coding and High Speed: cache-hit, cache-miss and output rates, alias mapping and apiToken.sale's 50% discount.",
    keywords: ["kimi api pricing", "kimi k3 price", "kimi for coding price", "kimi token cost", "kimi k2.7 code price", "cheap kimi api"],
    dek: "Kimi publishes cache-hit, cache-miss and output rates rather than one input price. apiToken.sale prices the model actually served, keeps those usage legs disjoint, and applies a flat 50% discount.",
    sections: [
      { h2: "Official rates behind the public aliases", blocks: [
        { type: "table", headers: ["Public alias", "Official hit / miss / output", "Price here after 50%"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "p", text: "Figures are per 1M tokens. Kimi caching is automatic. The provider publishes no separate cache-write price, so a newly cached token is a cache miss rather than a free or hidden fourth leg." },
      ] },
      { h2: "How to control spend", blocks: [
        { type: "list", items: [
          "Use Kimi for Coding for the lowest general coding rate in the published Kimi set.",
          "Use High Speed only when latency justifies exactly double the base token rates.",
          "Use k3-256k instead of the full 1M spelling when the task does not need the larger context mode.",
          "Set a lifetime key spending limit and inspect settled usage in the dashboard.",
        ] },
        { type: "note", text: "Reasoning tokens are a subset of output and bill at the output rate. They are not added again as a separate token class." },
      ] },
    ],
    faq: [
      { q: "How much does Kimi for Coding cost?", a: "Official replacement rates are $0.19 per 1M cache-hit tokens, $0.95 per 1M cache-miss tokens and $4 per 1M output tokens; apiToken.sale charges half." },
      { q: "Why are there cache-hit and cache-miss prices?", a: "Kimi automatically caches repeated context. Terminal usage identifies which input was served from cache, and each leg gets its own official rate." },
      { q: "Does High Speed cost more?", a: "Yes. Its cache-hit, cache-miss and output rates are exactly double the base Kimi for Coding rates." },
    ],
    related: ["kimi-k3-vs-kimi-for-coding", "how-to-buy-kimi-api-key", "kimi-api-quickstart", "how-billing-works"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
