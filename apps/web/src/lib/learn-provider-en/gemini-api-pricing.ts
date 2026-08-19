import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";
import { ROUTER } from "./shared";

export const article: LearnArticle = {
  slug: "gemini-api-pricing",
  cluster: "explain",
  title: "Gemini API Pricing Explained",
  h1: "Gemini API pricing: Pro, Flash, Flash-Lite and image output",
  description: "Gemini API pricing explained: Pro, Flash and Flash-Lite token rates, cached input, the 200K context premium and a flat 50% discount on apiToken.sale.",
  keywords: ["gemini api pricing", "gemini api cost per token", "gemini 3.6 flash price", "gemini 3.1 pro price", "gemini flash lite price", "gemini cached input pricing", "gemini long context pricing", "nano banana 2 api price", "gemini image output cost", "cheapest gemini model", "gemini api price per 1m tokens", "cheap gemini api"],
  dek: "Gemini API pricing is three metered legs — input, cached input and output — with rates set by model tier, a long-context premium on Pro, and a separate image-output leg on Nano Banana 2. This guide lists every current rate, the arithmetic for combining the legs, and where apiToken.sale's flat 50% discount enters settlement.",
  sections: [
    { h2: "Gemini API price per 1M tokens, model by model", blocks: [
      { type: "p", text: "Gemini API pricing is pure per-token metering: you pay for the input tokens you send and the output tokens the model generates, cached input bills as a cheaper leg of its own, and there is no per-request fee or minimum spend. Officially the spread runs from $0.10/$0.40 per 1M tokens on Gemini 2.5 Flash-Lite up to $2/$12 on Gemini 3.1 Pro Preview. apiToken.sale settles every one of those legs at a flat 50% discount, so the same requests cost $0.05/$0.20 to $1/$6." },
      { type: "table", headers: ["Model", "Official input / cached / output", "Price here after 50%"], rows: [
        ["gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
        ["gemini-3.6-flash", "$1.50 / $0.15 / $7.50", "$0.75 / $0.075 / $3.75"],
        ["gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
        ["gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
      ] },
      { type: "p", text: "All figures are per 1M tokens. Cached input is an independent usage leg reported in the response usage metadata: on the text models it bills at 10% of the fresh input rate, it applies automatically on repeated prompt prefixes, and there is no separate cache-write charge. You are never billed cached and fresh input for the same token." },
    ] },
    { h2: "How the token legs add up on a real call", blocks: [
      { type: "p", text: "The cost of any call is a sum of three multiplications: fresh input tokens times the input rate, cached tokens times the cached rate, and output tokens times the output rate. A gemini-3.6-flash request that sends 20,000 input tokens — 12,000 of them served from cache — and generates 1,500 output tokens costs 8,000 × $1.50/M + 12,000 × $0.15/M + 1,500 × $7.50/M = $0.012 + $0.0018 + $0.011 ≈ $0.025 at official rates. After the flat 50% discount that call settles at roughly $0.0125." },
      { type: "list", items: [
        "Output is the expensive leg: on every text model above, output costs 4–6× the input rate, so verbose replies cost more than long prompts.",
        "Model choice beats prompt trimming: 3.1 Flash-Lite input costs an eighth of 3.1 Pro input, and 2.5 Flash-Lite a twentieth.",
        "Stable prefixes — system prompts, tool schemas, few-shot examples — bill at 10% once cached, so repeat traffic gets cheaper without code changes.",
      ] },
      { type: "note", text: "The usageMetadata object in every generateContent response reports prompt, cached and candidate token counts separately. Budget from those authoritative numbers, not from character counts of your prompt." },
    ] },
    { h2: "Long-context pricing above 200K input tokens", blocks: [
      { type: "p", text: "Gemini 3.1 Pro Preview is the one text model with a long-context premium. Once a request crosses 200K input tokens, the entire request — not just the tokens past the threshold — bills at $4 input, $0.40 cached input and $18 output per 1M: double the input rate and 1.5× the output rate." },
      { type: "p", text: "The Flash and Flash-Lite models have no such tier. They hold their standard rates across the full 1M-token context window with up to 64K output. A 500K-token analysis that costs $2 in input alone on Pro costs $0.75 on gemini-3.6-flash — and the discount then halves both figures." },
      { type: "note", text: "Measure before sending a giant context to Pro: countTokens returns the exact billable input count for free (see below), so you can route oversized jobs to Flash deliberately instead of discovering the premium on the dashboard." },
    ] },
    { h2: "Image output: Gemini 3.1 Flash Image (Nano Banana 2)", blocks: [
      { type: "p", text: "Nano Banana 2 is the public name for gemini-3.1-flash-image, and it prices differently from the text models. Text in and out is cheap — $0.50 input and $3 text output per 1M — but rendered images bill as a separate leg at $60 per 1M image-output tokens. The context window is also smaller: 128K in, up to 32K out." },
      { type: "code", code: "curl " + ROUTER + "/v1beta/models/gemini-3.1-flash-image:generateContent \\\n  -H \"x-goog-api-key: $APITOKEN_API_KEY\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"contents\":[{\"parts\":[{\"text\":\"A clean isometric diagram of a satellite\"}]}]}'" },
      { type: "list", items: [
        "Image output meters per image-output token: $60 per 1M officially, $30 after the discount.",
        "Cached input gets no discount on this model — it bills at the full $0.50 input rate.",
        "Text output in the same response stays at the standard $3 per 1M rate.",
      ] },
      { type: "link", text: "Per-model detail: context, output limits and every rate leg", href: "/models/gemini-3-1-flash-image" },
    ] },
    { h2: "Estimate spend with countTokens before generating", blocks: [
      { type: "p", text: "countTokens is a free call on the same model path. It returns the exact input token count a request would bill, generates nothing and touches neither quota nor balance. Run it before large Pro or image jobs so the long-context premium never surprises you." },
      { type: "code", code: "curl " + ROUTER + "/v1beta/models/gemini-3.1-pro-preview:countTokens \\\n  -H \"x-goog-api-key: $APITOKEN_API_KEY\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"contents\":[{\"parts\":[{\"text\":\"<your full prompt here>\"}]}]}'" },
      { type: "p", text: "Streaming does not change the price either: streamGenerateContent?alt=sse meters the same token legs as a one-shot generateContent call, so pick the transport by latency and UX, not by cost." },
    ] },
    { h2: "How apiToken.sale settles Gemini usage at 50% off", blocks: [
      { type: "p", text: "None of the metering above changes on apiToken.sale. Your request runs on the native Gemini /v1beta generateContent surface, authenticated with your key in the x-goog-api-key header, and the usage metadata reports the same token counts. What changes is settlement: each call is first converted to exact official Google spend, then the flat 50% B2C discount is subtracted, and only the net amount leaves your prepaid balance. There is no subscription, no seat fee and no markup." },
      { type: "p", text: "The same key and balance cover supported Claude, GPT, Gemini and Kimi models, each metered against its own official rate card with the same discount. Every request appears in the dashboard with token-level detail, so you can reconcile the arithmetic in this guide against your real traffic." },
      { type: "link", text: "Every supported model with its per-token rates", href: "/models" },
      cta(),
    ] },
  ],
  faq: [
    { q: "What is the cheapest Gemini API model?", a: "Gemini 2.5 Flash-Lite at $0.10 input and $0.40 output per 1M tokens officially, with cached input at $0.01. With the flat 50% apiToken.sale discount that is $0.05/$0.20 — the lowest published per-token Gemini price." },
    { q: "When does Gemini long-context pricing apply?", a: "Only on Gemini 3.1 Pro Preview, once input exceeds 200K tokens. The whole request then bills at $4/$0.40/$18 per 1M — 2× input and 1.5× output. Flash and Flash-Lite keep their standard rates across the full 1M-token window." },
    { q: "How much does Gemini image output cost?", a: "Gemini 3.1 Flash Image (Nano Banana 2) bills rendered output at $60 per 1M image-output tokens officially, or $30 after the flat 50% discount. Text output in the same response bills at $3 per 1M." },
    { q: "Is cached input charged on top of fresh input?", a: "No. Cached tokens are a separate leg at 10% of the input rate on the text models, reported independently in usageMetadata — the same tokens are never billed twice. The exception is gemini-3.1-flash-image, where cached input bills at the full input rate." },
    { q: "Does the 50% discount apply to long-context and image legs?", a: "Yes. apiToken.sale computes the exact official spend for every leg — input, cached input, output, the Pro long-context premium and image output — and then subtracts 50% from the total before charging your prepaid balance." },
    { q: "Can I check a prompt's token count for free?", a: "Yes. POST to /v1beta/models/{model}:countTokens with your key in the x-goog-api-key header; it returns the exact input count and does not touch your balance." },
  ],
  related: ["gemini-pro-vs-flash-vs-flash-lite", "how-to-buy-gemini-api-key", "nano-banana-2-api-guide", "how-billing-works"],
  published: "2026-08-09",
  updated: "2026-08-17",
};
