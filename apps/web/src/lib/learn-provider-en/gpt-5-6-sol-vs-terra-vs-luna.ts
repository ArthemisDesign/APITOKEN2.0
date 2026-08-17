import type { LearnArticle } from "../learn";

export const article: LearnArticle = {
    slug: "gpt-5-6-sol-vs-terra-vs-luna",
    cluster: "compare",
    title: "GPT-5.6 Sol vs Terra vs Luna",
    h1: "GPT-5.6 Sol, Terra and Luna compared",
    description: "Compare GPT-5.6 Sol, Terra and Luna by price, reasoning effort, context and best use case, then choose the right GPT model for coding and production workloads.",
    keywords: ["gpt-5.6 sol vs terra", "gpt-5.6 terra vs luna", "best gpt-5.6 model", "gpt-5.6 models", "gpt-5.6 comparison", "gpt model for coding"],
    dek: "The GPT-5.6 family shares a 400K context window, 128K maximum output and the full reasoning-effort range. The practical difference is how much capability and latency you buy per token.",
    sections: [
      { h2: "Choose by workload", blocks: [
        { type: "table", headers: ["Tier", "Best fit", "Official input / output"], rows: [
          ["Sol", "Hard reasoning, long-horizon agents, difficult code review", "$5 / $30"],
          ["Terra", "Everyday coding, production chat, balanced agents", "$2 / $12"],
          ["Luna", "Classification, extraction, routing, high-volume simple work", "$0.20 / $1.20"],
        ] },
        { type: "p", text: "Terra is the safest default: it keeps Sol's controls and context at 40% of the token price. Escalate to Sol when evals show a quality gap; send predictable bulk work to Luna." },
      ] },
      { h2: "What stays the same", blocks: [
        { type: "list", items: [
          "400K context and up to 128K output.",
          "Text and image input with text output.",
          "Responses and Chat Completions, both with SSE streaming.",
          "Reasoning effort from none through max on the GPT-5.6 line.",
          "One endpoint, key and balance, so a router can switch models per task.",
        ] },
      ] },
    ],
    faq: [
      { q: "Which GPT-5.6 model is best for coding?", a: "Start with Terra for day-to-day coding. Use Sol for the hardest architecture or agentic tasks and Luna for cheap deterministic sub-steps." },
      { q: "Do Sol, Terra and Luna use different endpoints?", a: "No. All three use the same OpenAI-compatible base URL and key; only the model ID changes." },
      { q: "Does Terra support the max reasoning effort?", a: "Yes. Sol, Terra and Luna expose the same GPT-5.6 reasoning-effort set, including max." },
    ],
    related: ["gpt-api-pricing", "openai-api-quickstart", "codex-cli-setup", "gpt-image-2-api-guide"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
