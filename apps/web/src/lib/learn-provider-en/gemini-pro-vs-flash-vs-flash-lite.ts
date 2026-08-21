import type { LearnArticle } from "../learn";
import { ROUTER } from "./shared";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
    slug: "gemini-pro-vs-flash-vs-flash-lite",
    cluster: "compare",
    title: "Gemini Pro vs Flash vs Flash-Lite: Which to Pick",
    h1: "Gemini Pro vs Flash vs Flash-Lite: pick the right tier per request",
    description: "Gemini Pro vs Flash vs Flash-Lite compared: real input rates from $0.10 to $2 per 1M, context behavior, cache pricing — all three tiers on one key at 50% off.",
    keywords: ["gemini pro vs flash", "gemini flash vs flash lite", "gemini pro vs flash vs flash lite", "which gemini model to use", "best gemini model for coding", "gemini model comparison", "gemini 3.6 flash vs 3.1 pro", "gemini flash lite use cases", "gemini api model routing", "gemini tier pricing comparison", "cheapest gemini model"],
    dek: "The Gemini Pro vs Flash vs Flash-Lite decision is a routing problem, not a loyalty choice. Gemini 3.6 Flash is the default for coding and agents, Gemini 3.1 Pro Preview is the escalation tier for hard reasoning, and Gemini 3.1 Flash-Lite absorbs cheap bulk steps — all three on one key, one endpoint and one prepaid balance.",
    sections: [
      { h2: "The short answer: Flash by default, Pro on evidence, Flash-Lite on volume", blocks: [
        { type: "p", text: "Run Gemini 3.6 Flash as your default tier, escalate to Gemini 3.1 Pro Preview when a task genuinely needs deeper reasoning, and push deterministic bulk work down to Gemini 3.1 Flash-Lite. All three text tiers expose the same 1M-token context, the same 64K output ceiling and the same generateContent request shape, so the tier choice costs you one field per request — never a new integration." },
        { type: "p", text: "The expensive mistakes sit at both extremes. Running everything on Pro means paying Pro output rates for work Flash finishes just as well; never leaving Flash-Lite means retry loops on tasks it was never going to solve. Treat the three tiers as one system with different meters: Flash drafts, Pro handles the exceptions, Flash-Lite does the mechanical pre-processing in front of both." },
      ] },
      { h2: "The rate card: what each tier actually costs per million tokens", blocks: [
        { type: "p", text: "Pricing is where the tiers differ most. All figures below are per 1M tokens, shown as input / cached input / output, with the official Google rates and the effective price on apiToken.sale after the flat 50% B2C discount, which applies identically to every tier." },
        { type: "table", headers: ["Tier", "Model ID", "Official in / cached / out", "After 50% discount"], rows: [
          ["Pro", "gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["Flash", "gemini-3.6-flash", "$0.75 / $0.075 / $3.75 promo", "$0.375 / $0.0375 / $1.875"],
          ["Flash-Lite", "gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
          ["Flash-Lite (2.5)", "gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
        ] },
        { type: "p", text: "Two things stand out. Output is the expensive leg on every tier — four to eight times the input rate — so the model that finishes in one pass at a lower output rate usually beats a stronger model that needs retries. And the spread is enormous: Flash-Lite output bills at one eighth of Pro output, which is why routing classification and extraction away from the top tiers matters more than any prompt optimization." },
        { type: "note", text: "Gemini 3.6 Flash uses Google's $0.75/$0.075/$3.75 promotion through 2026-12-31 and returns to $1.50/$0.15/$7.50 on 2027-01-01. Cached input is a separate usage leg at 10% of fresh input; it is never added on top of fresh input for the same tokens." },
      ] },
      { h2: "Context window, output ceiling and the 200K threshold", blocks: [
        { type: "p", text: "Context is mostly not a differentiator: the current Pro, Flash and Flash-Lite text models all expose a 1M-token window and up to 64K output tokens. Flash-Lite is not a small-context tier — its advantage is cost and latency on simpler work, not a shorter window. The one context rule that changes your bill lives on Pro." },
        { type: "list", items: [
          "Gemini 3.1 Pro Preview requests above 200K input tokens reprice the whole request at $4 input and $18 output per 1M — the higher rates apply to every token, not just the overflow. After the 50% discount that is $2/$9.",
          "Flash and Flash-Lite keep flat rates across their full window; a 900K-input Flash call bills at the same per-token rate as a 1K call.",
          "The image tier is a different shape: Gemini 3.1 Flash Image exposes 128K context and up to 32K output, and its cached input bills at the full input rate rather than the text-model 10%.",
          "Before a large call, run countTokens on the same model path — it is free and tells you whether the request crosses the Pro 200K threshold before you pay for it.",
        ] },
      ] },
      { h2: "Which tier fits which workload", blocks: [
        { type: "p", text: "Map tiers to failure cost, not to vibes. A tier is right when the cost of a wrong or shallow answer on that step is lower than the token premium of the next tier up." },
        { type: "list", items: [
          "Pro (gemini-3.1-pro-preview): multi-file refactors, architecture and design trade-off analysis, deep document review, and final audit passes over Flash-generated output — work where a missed edge case costs more than the tokens.",
          "Flash (gemini-3.6-flash): everyday interactive coding, agent loops with many tool calls, multimodal inputs, and balanced production traffic. This is the correct default for roughly everything you have not measured otherwise.",
          "Flash-Lite (gemini-3.1-flash-lite): classification, extraction, routing, summarization and other deterministic pre-processing at volume, where the request is predictable and the quality bar is verifiable programmatically.",
          "Image (gemini-3.1-flash-image): any response that must contain a rendered image. Text output bills at $3 per 1M and image output at $60 per 1M image tokens ($1.50 and $30 after the discount), so never use it for text-only work.",
        ] },
        { type: "p", text: "The older Gemini 2.5 Flash-Lite remains in the catalog at $0.10/$0.40 official — the cheapest published text tier — and is a legitimate pick for high-volume pipelines already validated against it." },
      ] },
      { h2: "Switching tiers is a one-field change on one key", blocks: [
        { type: "p", text: `There is no per-tier plan, signup or endpoint. One apiToken.sale key covers every Gemini tier — plus the supported Claude, GPT and Kimi models — against a single prepaid balance. Point the native Gemini protocol at ${ROUTER}, send the key as x-goog-api-key, and change only the model ID:` },
        { type: "code", code: "curl " + ROUTER + "/v1beta/models/gemini-3.6-flash:generateContent \\\n  -H \"x-goog-api-key: $APITOKEN_API_KEY\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"contents\":[{\"parts\":[{\"text\":\"Summarize this diff in one sentence.\"}]}]}'" },
        { type: "p", text: "Swap gemini-3.6-flash for gemini-3.1-pro-preview or gemini-3.1-flash-lite and the identical request runs on that tier. GET /v1beta/models on the same base URL returns the exact IDs your key can call. Streaming works per tier via streamGenerateContent?alt=sse, and the official Google GenAI SDKs work unchanged apart from the base URL." },
        { type: "note", text: "SDK pitfall: pass the bare host as the base URL. The Google SDK appends /v1beta itself, so a base URL that already ends in /v1beta produces a doubled path and a 404." },
      ] },
      { h2: "A routing policy that keeps Gemini spend flat", blocks: [
        { type: "steps", items: [
          "Default every workload to gemini-3.6-flash — interactive sessions, CI agents and production traffic alike.",
          "Define escalation triggers in advance: a failed or shallow Flash attempt, a diff spanning more files than you can review by eye, or an irreversible design decision goes to gemini-3.1-pro-preview.",
          "Move deterministic sub-tasks — intent classification, field extraction, reranking — to gemini-3.1-flash-lite and verify output programmatically so quality regressions surface immediately.",
          "Call countTokens before any Pro request you suspect is near 200K input; if it crosses, either trim the context or accept the $4/$18 long-context rates deliberately.",
          "Review token-level usage in the dashboard weekly and adjust the split — the eight-fold gap between Flash-Lite and Pro output means a small shift in routing moves the bill more than any prompt tweak.",
        ] },
        { type: "p", text: "Because the 50% discount applies flat across tiers, the relative ranking never shifts — Flash-Lite is always the cheapest meter and Pro always the premium one, so a routing policy tuned on official prices stays valid here." },
        { type: "link", text: "Full Gemini rate card including image output and long-context legs", href: "/docs/learn/gemini-api-pricing" },
        { type: "link", text: "Compare every supported model and price", href: "/models" },
        cta(),
      ] },
    ],
    faq: [
      { q: "Which Gemini model should I use for coding?", a: "Start with Gemini 3.6 Flash — it is the best balance of quality, speed and price for interactive coding and agent loops. Escalate hard architecture and review work to Gemini 3.1 Pro Preview, and use Flash-Lite for cheap deterministic sub-tasks." },
      { q: "Is Flash-Lite limited to a smaller context window?", a: "No. The published text Flash-Lite models retain the same 1M-token context and 64K output ceiling as Flash and Pro. Their advantage is lower cost and latency on simpler work, not a shorter window." },
      { q: "When does Gemini Pro long-context pricing apply?", a: "When a Gemini 3.1 Pro Preview request exceeds 200K input tokens, the entire request reprices to $4 input and $18 output per 1M officially ($2/$9 after the 50% discount). Flash and Flash-Lite have no long-context premium. Run the free countTokens call first if you are unsure." },
      { q: "Can I switch between Pro, Flash and Flash-Lite without a new key?", a: "Yes. Keep the same base URL and x-goog-api-key header and change only the model ID in the generateContent path. One key and one prepaid balance cover all Gemini tiers plus the supported Claude, GPT and Kimi models." },
      { q: "Does the apiToken.sale discount apply to all three tiers?", a: "Yes. The flat 50% B2C discount is applied after the exact official usage legs — input, cached input, output and any long-context or image legs — are calculated, identically across Pro, Flash, Flash-Lite and Flash Image." },
      { q: "What is the cheapest Gemini model for high-volume work?", a: "Gemini 2.5 Flash-Lite at $0.10/$0.40 per 1M tokens officially ($0.05/$0.20 after the discount), with Gemini 3.1 Flash-Lite at $0.25/$1.50 official as the current-generation budget tier." },
    ],
    related: ["gemini-api-pricing", "gemini-api-quickstart", "nano-banana-2-api-guide", "best-claude-model-for-coding"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
