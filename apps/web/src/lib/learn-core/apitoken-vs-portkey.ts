import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "apitoken-vs-portkey",
  cluster: "compare",
  title: "apiToken.sale vs Portkey for Claude",
  h1: "apiToken.sale vs Portkey: key supplier vs AI gateway",
  description: "Portkey alternative for Claude: Portkey routes keys you already own; apiToken.sale supplies the Claude key itself at 50% off. When to use each — or both.",
  keywords: ["portkey alternative", "apitoken vs portkey", "portkey claude api", "ai gateway claude", "claude api gateway", "byok ai gateway", "portkey anthropic provider", "anthropic api alternative", "claude api discount", "cheap claude api", "best claude api"],
  dek: "People searching for a Portkey alternative usually want one of two things: cheaper Claude tokens, or gateway features without Anthropic billing. Portkey solves the second problem; apiToken.sale solves the first. This guide shows how to tell which one you need — and how to run both together.",
  sections: [
    { h2: "Portkey manages keys you already own — it does not sell them", blocks: [
      { type: "p", text: "apiToken.sale and Portkey are not two flavors of the same product. Portkey is an AI gateway: it sits in front of provider API keys you already own and adds routing, caching and observability on top. apiToken.sale is where the Claude key and balance come from in the first place — a native Anthropic endpoint with a flat 50% discount and no Anthropic account required." },
      { type: "p", text: "That distinction decides everything else. With Portkey alone you still bring a funded Anthropic account, which means passing Anthropic's own sign-up, billing-country and payment checks, and paying full official token rates. The gateway changes how your requests travel; it never changes what the provider behind it charges per token." },
    ] },
    { h2: "What an AI gateway earns its keep doing", blocks: [
      { type: "p", text: "If you operate several provider accounts or run production traffic, the gateway layer is genuinely useful. The feature set is about control, not price:" },
      { type: "list", items: [
        "Fallbacks and load balancing across targets when a provider errors or rate-limits.",
        "Automatic retries, plus response caching for repeated prompts.",
        "Request logs, traces and usage analytics across every provider you connect.",
        "Guardrails and virtual keys, so teammates and services get scoped credentials instead of your raw provider key.",
      ] },
      { type: "p", text: "Portkey's gateway is open source, so you can self-host it next to your app, or use the hosted cloud and skip the ops work. Either way, the model tokens themselves are billed by whichever provider account sits behind the gateway — that bill is exactly what a gateway cannot improve." },
    ] },
    { h2: "Where the discounted Claude key actually comes from", blocks: [
      { type: "p", text: `apiToken.sale is the supplier side of the stack. You top up a prepaid balance — any whole-dollar amount, by bank card or cryptocurrency — and call the standard Anthropic Messages API at ${BASE} with a key that looks like ${KEY}. Every request is metered at official Anthropic rates, then a flat 50% B2C discount is subtracted before the charge touches your balance. The balance never expires, and no Anthropic account is involved at any point.` },
      { type: "table", headers: ["Model", "Official in / out ($ per 1M)", "Here (−50%)"], rows: [
        ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
        ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
        ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
      ] },
      { type: "p", text: "Because the endpoint speaks the native Messages API, Claude Code, Cursor and the official Anthropic SDKs work with a two-line change: the base URL and the key. One key also covers supported GPT, Gemini and Kimi models on their own protocols, so the same balance follows you across providers." },
      { type: "link", text: "Full per-model pricing, including cache rates", href: "/models" },
      { type: "link", text: "Estimate your monthly spend in the free calculator", href: "/tools/claude-api-cost-calculator" },
      cta(),
    ] },
    { h2: "Point Portkey at an apiToken.sale key", blocks: [
      { type: "p", text: "The two products compose cleanly: Portkey keeps doing routing and observability, while the discounted apiToken.sale key sits underneath as the Anthropic provider. You keep the gateway's dashboards and fallbacks, and the spend it logs is already 50% lower." },
      { type: "steps", items: [
        `Create an apiToken.sale account and generate a key in the dashboard — it looks like ${KEY} and works on every supported Claude model.`,
        `In Portkey, add an Anthropic target and override its base URL with a custom host pointing at ${BASE}, then paste the sk-pool key as the credential.`,
        "Send your application traffic through Portkey as usual. Requests arrive at the apiToken.sale endpoint in standard Anthropic Messages format, so model IDs, streaming and prompt caching behave exactly as they would against Anthropic.",
      ] },
      { type: "code", code: `// Portkey gateway config: Anthropic provider, discounted endpoint underneath
{
  "targets": [
    {
      "provider": "anthropic",
      "api_key": "${KEY}",
      "custom_host": "${BASE}",
      "override_params": { "model": "claude-sonnet-5" }
    }
  ]
}` },
      { type: "note", text: "Keep real Anthropic model IDs (claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5) in the target — the endpoint behind the custom host is the native Messages API, not a translated shape. The exact override field lives on the target in gateway configs and on the provider credential in the hosted dashboard; the idea is the same: Portkey forwards Anthropic-format calls to the apiToken.sale base URL." },
    ] },
    { h2: "Same traffic, two different bills", blocks: [
      { type: "p", text: "Take a realistic month of agentic coding on claude-sonnet-5: 10M input tokens and 2M output tokens. At official rates of $3 / $15 per million, that is $30 + $30 = $60. Routing that traffic through a gateway with your own Anthropic key leaves the $60 untouched — you get better logs, not a smaller invoice. The same traffic on an apiToken.sale key costs $30, because the discount applies at the key supplier, where the metering happens." },
      { type: "list", items: [
        "Gateway alone: full official provider bill, plus routing and observability.",
        "Discounted key alone: half the bill, called directly with no extra hop.",
        "Gateway in front of the discounted key: half the bill and the gateway's controls.",
      ] },
    ] },
    { h2: "Which layer do you actually need?", blocks: [
      { type: "list", items: [
        "You just want cheaper Claude with working tools — apiToken.sale alone. Change the base URL and key, done.",
        "You already fund several provider accounts and need fallbacks, tracing and guardrails across them — Portkey alone, and accept official token prices.",
        "You want production controls and a lower Claude bill — run Portkey in front of an apiToken.sale key as described above.",
      ] },
      { type: "p", text: "Most individual developers and small teams comparing the two are in the first group: the pain is the Anthropic account and the token price, not a missing routing layer. Start with the discounted key, and add a gateway only when multi-provider operations actually demand one." },
    ] },
  ],
  faq: [
    { q: "Does Portkey give me a Claude API discount?", a: "No. Portkey is a gateway over keys you already own, so you keep paying your provider's official rates. The discounted Claude key and balance come from apiToken.sale, which meters official spend and subtracts a flat 50% B2C discount." },
    { q: "Can I use Portkey and apiToken.sale together?", a: `Yes. Add an Anthropic target in Portkey, override its base URL with ${BASE} as the custom host, and paste your sk-pool key — you keep Portkey's observability while the spend underneath is discounted.` },
    { q: "Do I still need an Anthropic account if I use Portkey?", a: "With Portkey alone, yes — it routes requests through provider keys you bring, so a funded Anthropic account sits behind it. With an apiToken.sale key, no Anthropic account is required at all." },
    { q: "Is Portkey a Claude API provider?", a: "No. It never sells model access or token balance; it is a control layer between your application and providers you pay directly. apiToken.sale is the opposite: it supplies the key and prepaid balance and adds no routing layer of its own." },
    { q: "Will my Anthropic SDK code still work if I switch key suppliers?", a: `Yes. apiToken.sale serves the native Anthropic Messages API at ${BASE}, so the official SDKs, Claude Code and Cursor keep working — you change only the base URL and the API key.` },
  ],
  related: ["apitoken-vs-openrouter", "claude-api-gateway", "cheapest-claude-api", "anthropic-sdk-base-url"],
  updated: "2026-08-17",
};
