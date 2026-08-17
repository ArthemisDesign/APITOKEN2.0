import type { LearnArticle } from "../learn";

export const article: LearnArticle = {
    slug: "kimi-k3-vs-kimi-for-coding",
    cluster: "compare",
    title: "Kimi K3 vs Kimi for Coding",
    h1: "Kimi K3 and Kimi for Coding compared",
    description: "Compare Kimi K3, K3 256K, Kimi for Coding and High Speed by context, reasoning controls, latency and token price for coding and agent workloads.",
    keywords: ["kimi k3 vs kimi for coding", "kimi k3 api", "kimi k2.7 code", "best kimi model for coding", "kimi models comparison", "kimi highspeed"],
    dek: "K3 is the reasoning and long-context family; Kimi for Coding is the economical coding family. High Speed buys latency at double the rate, while K3's aliases choose a 256K or 1M context mode.",
    sections: [
      { h2: "Model-family map", blocks: [
        { type: "table", headers: ["Public ID", "Context", "Best fit"], rows: [
          ["kimi/kimi-for-coding", "256K", "Everyday coding and economical agent loops"],
          ["kimi/kimi-for-coding-highspeed", "256K", "Latency-sensitive coding where speed pays for itself"],
          ["kimi/k3-256k", "256K", "K3 reasoning without the full-context mode"],
          ["kimi/k3 · kimi/k3[1m]", "1M", "Long codebases, documents and hard reasoning"],
        ] },
        { type: "p", text: "k3[1m] is a compatibility spelling of K3's 1M mode, not a separately priced model. The router normalizes it to the provider's real k3 wire model." },
      ] },
      { h2: "Reasoning and routing", blocks: [
        { type: "list", items: [
          "K3 supports low, high and max reasoning effort; high is the default.",
          "Kimi for Coding and High Speed run with thinking enabled.",
          "Model access is catalog-driven, so check the scoped /v1/models response before pinning an alias.",
          "A practical router sends everyday code to Kimi for Coding and escalates large or difficult work to K3.",
        ] },
      ] },
    ],
    faq: [
      { q: "Which Kimi model is best for coding?", a: "Kimi for Coding is the economical default. Choose K3 for harder reasoning or long-context codebase work, and High Speed only when lower latency is worth double rates." },
      { q: "Are k3 and k3[1m] different models?", a: "No. They select the same K3 1M mode; the bracket form is a compatibility alias." },
      { q: "Can I request Kimi's internal official model IDs?", a: "No. Use the public subscription aliases returned by the router catalog, not internal tariff IDs such as kimi-k2.7-code." },
    ],
    related: ["kimi-api-pricing", "kimi-api-quickstart", "kimi-api-for-claude-code", "how-to-buy-kimi-api-key"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
