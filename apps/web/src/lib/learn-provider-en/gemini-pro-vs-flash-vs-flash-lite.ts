import type { LearnArticle } from "../learn";

export const article: LearnArticle = {
    slug: "gemini-pro-vs-flash-vs-flash-lite",
    cluster: "compare",
    title: "Gemini Pro vs Flash vs Flash-Lite",
    h1: "Gemini Pro, Flash and Flash-Lite compared",
    description: "Compare Gemini Pro, Flash and Flash-Lite by price, context, reasoning and best use case. Choose the right Gemini model for coding, agents and high-volume API work.",
    keywords: ["gemini pro vs flash", "gemini flash vs flash lite", "best gemini model", "gemini models comparison", "gemini model for coding", "gemini 3.6 flash"],
    dek: "Use the tier as a routing decision, not a loyalty choice: Pro for the hardest reasoning, Flash as the coding default, and Flash-Lite for cheap high-volume steps. One key can use all three.",
    sections: [
      { h2: "Choose by task", blocks: [
        { type: "table", headers: ["Tier", "Best fit", "Recommended current ID"], rows: [
          ["Pro", "Hard reasoning, planning, deep codebase and document analysis", "gemini-3.1-pro-preview"],
          ["Flash", "Everyday coding, multimodal agents, balanced production traffic", "gemini-3.6-flash"],
          ["Flash-Lite", "Classification, extraction, routing and cheap pre-processing", "gemini-3.1-flash-lite"],
          ["Image", "Image generation and editing", "gemini-3.1-flash-image"],
        ] },
        { type: "p", text: "Gemini 3.6 Flash is the best starting point for most new text workloads. Move only the hardest calls to Pro and the most predictable bulk calls to Flash-Lite." },
      ] },
      { h2: "Context and cost trade-offs", blocks: [
        { type: "list", items: [
          "The current text models expose a 1M-token context and up to 64K output.",
          "Pro has a long-context premium above 200K input; Flash and Flash-Lite keep flat rates across their window.",
          "Cached input normally bills at 10% of fresh input on the text models.",
          "Use countTokens before very large calls and route by measured quality, not model name alone.",
        ] },
      ] },
    ],
    faq: [
      { q: "Which Gemini model should I use for coding?", a: "Start with Gemini 3.6 Flash. Escalate difficult architecture and review work to 3.1 Pro Preview; use Flash-Lite for cheap deterministic sub-tasks." },
      { q: "Is Flash-Lite limited to a smaller context?", a: "No. The published text Flash-Lite models retain the 1M-token context; their advantage is lower cost and latency for simpler work." },
      { q: "Can I switch tiers without a new key?", a: "Yes. Keep the same Gemini base URL and x-goog-api-key, and change only the model ID." },
    ],
    related: ["gemini-api-pricing", "gemini-api-quickstart", "nano-banana-2-api-guide", "best-claude-model-for-coding"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
