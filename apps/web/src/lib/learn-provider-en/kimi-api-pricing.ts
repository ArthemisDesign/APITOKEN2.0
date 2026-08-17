import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";
import { ROUTER } from "./shared";

export const article: LearnArticle = {
    slug: "kimi-api-pricing",
    cluster: "explain",
    title: "Kimi API Pricing Explained",
    h1: "Kimi API pricing: cache hits, misses, output and speed",
    description: "Understand Kimi API pricing for K3, Kimi for Coding and High Speed: cache-hit, cache-miss and output rates, alias mapping and apiToken.sale's 50% discount.",
    keywords: [
      "kimi api pricing",
      "kimi k3 price",
      "kimi for coding price",
      "kimi token cost",
      "kimi k2.7 code price",
      "cheap kimi api",
      "kimi api cost per million tokens",
      "kimi highspeed price",
      "kimi cache hit pricing",
      "moonshot kimi api cost",
    ],
    dek: "Kimi API pricing splits every request into three usage legs — cache-hit input, cache-miss input and output — each with its own official rate per million tokens. This guide walks the real rate card for K3, Kimi for Coding and High Speed, works through the arithmetic on a realistic coding session, and shows where apiToken.sale's flat 50% discount enters the settlement.",
    sections: [
      { h2: "The three legs every Kimi request is billed on", blocks: [
        { type: "p", text: "Kimi does not publish a single input price. Every request is metered as cache-hit input, cache-miss input and output, and each leg carries its own official rate per million tokens — cache hits cost a fraction of misses, and output costs the most. apiToken.sale settles those exact legs against the official rate card, then subtracts a flat 50% discount before the charge touches your prepaid balance." },
        { type: "p", text: "Caching itself is automatic on the provider side; you do not opt in, mark breakpoints or manage cache entries. When a request repeats context the provider has already seen, the matching prefix is served from cache and metered at the hit rate. Everything else is a miss. Because the provider publishes no separate cache-write price, there is no hidden fourth leg: a newly cached token is simply billed as a cache miss on the request that introduced it, and later requests collect the cheap hit rate." },
        { type: "p", text: "The practical consequence: two identical-looking calls can cost very different amounts depending on how much of the prompt hits cache. Agent loops that resend a large stable system prompt and file context every turn are exactly the traffic that harvests hit rates — the first turn pays misses, the rest ride the cache." },
        { type: "note", text: "Reasoning tokens are a subset of output, not a separate token class. Kimi for Coding and High Speed run with thinking enabled, and everything the model reasons through bills at the output rate — it is never added again on top of output." },
      ] },
      { h2: "Official rates behind each public alias", blocks: [
        { type: "table", headers: ["Public alias", "Official hit / miss / output", "Price here after 50%"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "p", text: "All figures are per 1M tokens, read left to right as cache-hit input, cache-miss input, output. Two structural facts fall out of the table. First, Kimi for Coding is the lowest general coding rate in the published Kimi set. Second, High Speed is exactly double the base Kimi for Coding rates on every leg — you are buying latency, not a different model." },
        { type: "p", text: "The K3 row covers three spellings of one rate card. k3-256k selects K3's 256K context mode; k3 and k3[1m] select the 1M mode, and the bracket form is a compatibility spelling the router normalizes to the provider's real K3 wire model. Per-token rates are identical across all three, so the choice is about context behavior, not price. What you must not send is an internal Open Platform tariff ID such as kimi-k2.7-code — the router accepts only the public subscription aliases returned by GET /v1/models." },
      ] },
      { h2: "Worked example: one coding session, leg by leg", blocks: [
        { type: "p", text: "Take a typical agent session on kimi/kimi-for-coding: a stable system prompt and repository context that cache after the first turn, some fresh input each turn, and compact replies. Assume the session settles at 400,000 cache-hit tokens, 100,000 cache-miss tokens and 20,000 output tokens:" },
        { type: "table", headers: ["Leg", "Calculation at official rates", "Amount"], rows: [
          ["Cache-hit input", "400,000 × $0.19 / 1M", "$0.076"],
          ["Cache-miss input", "100,000 × $0.95 / 1M", "$0.095"],
          ["Output", "20,000 × $4 / 1M", "$0.08"],
          ["Official spend", "$0.076 + $0.095 + $0.08", "$0.251"],
          ["Flat 50% B2C discount", "$0.251 × 50%", "−$0.1255"],
          ["Deducted from balance", "$0.251 − $0.1255", "$0.1255"],
        ] },
        { type: "p", text: "Two things are worth internalizing from the arithmetic. Output dominates despite being the smallest token count — twenty times fewer output tokens than cache hits still cost more than the entire hit leg. And the cache did most of the work: the same 500,000 input tokens billed entirely as misses would have cost $0.475 officially instead of $0.171. Keeping the expensive context stable across turns is the single biggest lever on a Kimi bill." },
      ] },
      { h2: "Choosing the cheapest alias for the job", blocks: [
        { type: "list", items: [
          "Default coding and agent loops to kimi/kimi-for-coding — the lowest general coding rate in the published set.",
          "Reach for kimi/kimi-for-coding-highspeed only when latency is worth exactly double the base rates on every leg; an idle-time-insensitive batch job never is.",
          "Use kimi/k3-256k instead of the 1M spelling when the task does not need the larger context mode — the per-token price is the same, so you are picking behavior, not a discount.",
          "Escalate to K3 ($0.30 / $3 / $15 official) only for work Kimi for Coding actually fails: hard reasoning or long-document and long-codebase tasks.",
          "Set a lifetime spending limit on the key and inspect settled usage in the dashboard instead of trusting end-of-month estimates.",
        ] },
      ] },
      { h2: "How settlement, top-ups and the balance work", blocks: [
        { type: "p", text: "apiToken.sale changes nothing about the metering above. Your request runs against Kimi's own usage accounting, each leg is converted to official provider spend, and only then is the flat 50% B2C discount subtracted. The net amount draws from one prepaid balance that also covers supported Claude, GPT and Gemini models against their own official rate cards. There is no subscription, no per-request fee and no minimum spend." },
        { type: "p", text: "You top up any whole-dollar amount by bank card or cryptocurrency, and the balance never expires. Every request appears in the dashboard with its model and token-level breakdown, so you can reconcile the arithmetic in this guide line by line against your real traffic. When the balance reaches zero, requests fail with an insufficient-balance error until you top up again — the key itself stays valid." },
        cta(),
      ] },
      { h2: "Reconcile the legs in the response usage object", blocks: [
        { type: "p", text: "You never have to estimate any of this from character counts. The terminal usage object reports what each call actually consumed, and Kimi follows the Anthropic Messages response shape — cached input, fresh input and output land in disjoint buckets you can multiply by the rate card directly:" },
        { type: "code", code: "curl " + ROUTER + "/v1/messages \\\n  -H \"x-api-key: $APITOKEN_API_KEY\" \\\n  -H \"anthropic-version: 2023-06-01\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"model\":\"kimi/kimi-for-coding\",\"max_tokens\":512,\"messages\":[{\"role\":\"user\",\"content\":\"Summarize this diff\"}]}'\n\n# The response carries terminal usage in the Anthropic shape:\n# \"usage\": {\n#   \"input_tokens\": 8420,\n#   \"cache_read_input_tokens\": 38400,\n#   \"output_tokens\": 512\n# }\n# cache_read_input_tokens bills at the hit rate ($0.19/1M),\n# input_tokens at the miss rate ($0.95/1M),\n# output_tokens at the output rate ($4/1M) — then 50% off the total." },
        { type: "p", text: "Because the legs stay disjoint all the way from the provider's accounting to your dashboard ledger, a cost anomaly always has an address: a jump in cache misses means someone edited the middle of a previously stable prefix, and a jump in output means the model started reasoning or replying longer. OpenAI-compatible clients reach the same aliases through the universal /v1 lane and settle identically." },
        { type: "link", text: "Per-model rates, cache legs and context windows", href: "/models" },
      ] },
    ],
    faq: [
      { q: "How much does Kimi for Coding cost?", a: "Official replacement rates are $0.19 per 1M cache-hit tokens, $0.95 per 1M cache-miss tokens and $4 per 1M output tokens; apiToken.sale charges half of each leg after settlement." },
      { q: "Why does Kimi have separate cache-hit and cache-miss prices?", a: "Kimi automatically caches repeated context. Terminal usage identifies which input was served from cache, and each leg gets its own official rate — hits are much cheaper than misses." },
      { q: "Is there a separate price for writing to Kimi's cache?", a: "No. The provider publishes no cache-write leg, so a newly cached token is billed as a cache miss on the request that introduced it; later requests pay the hit rate." },
      { q: "Does Kimi High Speed cost more?", a: "Yes. Its cache-hit, cache-miss and output rates are exactly double the base Kimi for Coding rates — $0.38 / $1.90 / $8 official per 1M tokens." },
      { q: "Are k3, k3-256k and k3[1m] priced differently?", a: "No. All three spellings share one rate card ($0.30 / $3 / $15 official per 1M); they only select K3's 256K or 1M context mode, and k3[1m] is a compatibility alias." },
      { q: "Do Kimi reasoning tokens cost extra?", a: "They bill at the output rate as a subset of output tokens. Thinking is never metered as a separate token class or added on top of output." },
    ],
    related: ["kimi-k3-vs-kimi-for-coding", "how-to-buy-kimi-api-key", "kimi-api-quickstart", "how-billing-works"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
