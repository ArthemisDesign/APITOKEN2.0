import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
    slug: "gpt-api-pricing",
    cluster: "explain",
    title: "GPT API Pricing Explained",
    h1: "GPT API pricing: input, cache, output and long context",
    description: "GPT API pricing explained: GPT-5.6 Sol, Terra and Luna rates per 1M tokens, cache pricing, the 272K long-context multiplier and a flat 50% discount.",
    keywords: ["gpt api pricing", "gpt api cost", "gpt-5.6 price per 1m tokens", "gpt token pricing", "gpt-5.6 sol price", "gpt cached input price", "gpt long context pricing", "gpt api cost per token", "openai api pricing explained", "cheapest gpt api"],
    dek: "GPT API pricing is a sum of exact token legs — fresh input, cached input, cache writes and output — each billed at the rate of the model tier that served the request. This guide walks the GPT-5.6 Sol, Terra and Luna rate card, the cache and long-context multipliers that move the bill, and the point where apiToken.sale's flat 50% discount enters settlement.",
    sections: [
      { h2: "How a GPT API bill is actually computed", blocks: [
        { type: "p", text: "GPT API pricing is per-token metering, not a price per request. Every call bills the tokens you send in — system prompt, conversation history, tool definitions, retrieved documents — at an input rate, and the tokens the model generates back at a separate, higher output rate. There is no per-request fee, seat license or minimum spend: multiply token counts by the model's rates and you have the exact cost of a call." },
        { type: "p", text: "A token is roughly four characters of English text, about three quarters of a word. Code, JSON and non-Latin scripts tokenize denser, so a source file costs more tokens than the same word count in prose. Output is priced higher for a mechanical reason: input is processed in parallel in one pass, while every output token requires its own forward pass through the model." },
        { type: "p", text: "You never have to estimate any of this from character counts. The response usage object reports exactly what a call consumed, split into the legs the rate card prices:" },
        { type: "code", code: `"usage": {
  "prompt_tokens": 40210,
  "completion_tokens": 1834,
  "total_tokens": 42044,
  "prompt_tokens_details": { "cached_tokens": 32000 }
}` },
        { type: "p", text: "Take gpt-5.6-sol at the temporary official rates of $4 per 1M input tokens, $0.40 per 1M cached input, $5 per 1M cache-write tokens and $20 per 1M output. The call above costs 8,210 × $4/M fresh input + 32,000 × $0.40/M cached + 1,834 × $20/M output = $0.08232 at official rates — and $0.04116 after the flat 50% discount here. That is arithmetic on authoritative usage numbers, which is why the usage object, not the length of your prompt string, is the source of truth for budgeting." },
      ] },
      { h2: "GPT-5.6 rate card: Sol, Terra and Luna", blocks: [
        { type: "table", headers: ["Model", "Official input / cached / cache write / output", "Price here after 50%"], rows: [
          ["gpt-5.6-sol", "$4 / $0.40 / $5 / $20", "$2 / $0.20 / $2.50 / $10"],
          ["gpt-5.6-terra", "$2 / $0.20 / $2.50 / $12", "$1 / $0.10 / $1.25 / $6"],
          ["gpt-5.6-luna", "$0.20 / $0.02 / $0.25 / $1.20", "$0.10 / $0.01 / $0.125 / $0.60"],
        ] },
        { type: "p", text: "Rates are per 1M tokens. Sol's temporary official promotion runs through 2026-11-21 inclusive; standard $5 input and $30 output rates return on 2026-11-22 UTC. gpt-5.6 is an alias of gpt-5.6-sol, so it has the same price rather than a separate tariff — switching between the two IDs changes nothing on the bill." },
        { type: "p", text: "The spread between tiers is the single biggest lever on GPT spend. At the temporary rates, Terra costs 50% of Sol input and 60% of Sol output, while Luna costs 5% and 6% respectively. Routing each task to the weakest tier that handles it well — Luna for classification, extraction and routing, Terra for everyday coding and production chat, Sol for the hardest reasoning — saves more than any amount of prompt trimming, and the 50% discount preserves the ranking because it applies equally to every leg." },
      ] },
      { h2: "Cache reads at 10%, cache writes at 125%", blocks: [
        { type: "p", text: "When a repeated prompt prefix is served from the provider's cache, those tokens bill as a separate cached-input leg at 10% of the normal input rate — $0.40 instead of $4 per 1M on Sol during the promotion. Storing a prefix is not free: GPT-5.6 cache writes bill at 125% of normal input, or $5 per 1M on promotional Sol, so the first call pays a small premium and every later hit is nearly free. Even a single re-read comes out ahead: 1.25× plus 0.10× is less than paying full input twice." },

        { type: "list", items: [
          "The terminal usage object decides which tokens were cached; you are never charged both cached and fresh input for the same token.",
          "Keep stable content — system prompt, tool definitions, large reference files — at the front of the prompt; the cache matches prefixes, not fragments.",
          "Editing the middle of a cached prefix invalidates everything from the edit point onward, and the next call pays full input price for that tail again.",
          "Multi-turn agents benefit automatically: each turn resends the accumulated conversation as input, and the unchanged earlier turns arrive as cached tokens.",
        ] },
        { type: "note", text: "The classic cache killer is volatile data at the top of the prompt — timestamps, request IDs, random seeds. Move anything that changes per call to the end of the message list, or every request becomes a fresh cache write at 125% with no cheap reads to harvest." },
      ] },
      { h2: "The 272K long-context cliff", blocks: [
        { type: "p", text: "Above 272K input tokens, GPT long-context pricing applies to the whole request: 2× on input and 1.5× on output, not just on the tokens past the boundary. The jump is sharp enough to matter in planning, because a request just under the threshold and one just over it differ by far more than the extra tokens." },
        { type: "p", text: "On promotional Sol, 270K input tokens with 2K output cost 270,000 × $4/M + 2,000 × $20/M = $1.12 official. Add 3K more input tokens — 273,000 × $8/M + 2,000 × $30/M — and the same request costs $2.244. One percent more input roughly doubles the bill. Trim conversation history, tighten retrieval, or split the job before crossing the boundary; after the 50% discount the cliff is still the cliff, just at half height." },
      ] },
      { h2: "Reasoning tokens bill as output, once", blocks: [
        { type: "p", text: "GPT-5.6 models expose an adjustable reasoning effort, and the reasoning they generate is billed inside output usage at the output rate. Reasoning tokens appear in the output leg and are not charged a second time as a separate token class — but they are real output tokens at the output price, which on promotional Sol is $20 per 1M official." },
        { type: "p", text: "That makes effort the second lever after model tier. A hard architecture problem can justify max effort on Sol; a routing or formatting sub-step cannot. Sending predictable bulk work to Luna at a low effort cuts both the per-token rate and the number of invisible tokens you pay for." },
      ] },
      { h2: "Where the flat 50% discount settles", blocks: [
        { type: "p", text: "apiToken.sale changes none of the mechanics above. Your request goes to an OpenAI-compatible endpoint, the same model IDs answer it, and the usage object reports the same token legs. What changes is settlement: each call is first converted to exact official spend — input, cached input, cache writes, output, and the long-context multipliers where they apply — then the flat 50% B2C discount is subtracted before anything touches your prepaid balance. There is no subscription and no markup; the discount is the pricing." },
        { type: "p", text: "The dashboard records the settled token usage and the exact discounted charge for each request, so you can reconcile the arithmetic in this guide against your real traffic call by call instead of trusting an end-of-month invoice. The same prepaid balance also meters supported Claude, Gemini and Kimi models against their own official rate cards, with the same discount applied." },
        { type: "link", text: "Per-model pages with full rate cards and context windows", href: "/models" },
        cta(),
      ] },
    ],
    faq: [
      { q: "How much does GPT-5.6 cost per 1M tokens?", a: "Through 2026-11-21 inclusive, Sol's temporary official input/cached/cache-write/output rates are $4/$0.40/$5/$20, or $2/$0.20/$2.50/$10 here after 50% off. Standard $5 input and $30 output return on 2026-11-22 UTC. Terra remains $2/$12 and Luna $0.20/$1.20." },
      { q: "What counts as cached input?", a: "Repeated prompt prefixes that the provider serves from cache. The terminal usage determines the cached leg; you are not charged both cached and fresh input for the same token." },
      { q: "When does long-context pricing start?", a: "When input exceeds 272K tokens. The whole request then bills at 2× input and 1.5× output before the 50% discount." },
      { q: "Are reasoning tokens billed separately?", a: "No. Reasoning tokens are part of output usage and bill at the model's output rate; they are not a second, separate leg on top of output." },
      { q: "Is the gpt-5.6 alias cheaper than gpt-5.6-sol?", a: "No. gpt-5.6 is an alias of gpt-5.6-sol and shares its exact rate card, so both IDs cost the same per token." },
    ],
    related: ["gpt-5-6-sol-vs-terra-vs-luna", "how-to-buy-gpt-api-key", "openai-api-quickstart", "save-tokens-on-claude-api"],
    published: "2026-08-09",
    updated: "2026-08-17",
  };
