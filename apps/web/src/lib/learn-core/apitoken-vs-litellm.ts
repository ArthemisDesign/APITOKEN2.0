import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "apitoken-vs-litellm",
  cluster: "compare",
  title: "apiToken.sale vs LiteLLM for Claude",
  h1: "apiToken.sale vs LiteLLM",
  description: "LiteLLM is a self-hosted proxy that unifies model APIs over keys you fund yourself. apiToken.sale is a hosted endpoint that sells the key and balance at a 50% discount. Compare both, or combine them.",
  keywords: ["litellm alternative", "apitoken vs litellm", "litellm claude", "self-hosted llm proxy", "litellm proxy vs hosted api", "claude api without self-hosting", "litellm api_base anthropic", "claude api discount", "hosted claude api endpoint", "cheap claude api"],
  dek: "Searching for a LiteLLM alternative usually means you want one of two things: a unified API layer without running a proxy, or cheaper Claude tokens. apiToken.sale answers both — a hosted endpoint where one prepaid key covers supported Claude, GPT, Gemini and Kimi models at a flat 50% B2C discount. LiteLLM still wins when you deliberately want to own the routing layer.",
  updated: "2026-08-17",
  sections: [
    { h2: "The short answer: a proxy you run vs an endpoint you point at", blocks: [
      { type: "p", text: "LiteLLM is software — an open-source proxy you deploy in front of provider accounts you fund yourself. apiToken.sale is a service — a hosted, prepaid endpoint where the key and the balance are the product. If your goal is discounted Claude access with zero infrastructure, LiteLLM alone cannot get you there; if your goal is owning a routing layer across many providers, apiToken.sale alone does not try to." },
      { type: "table", headers: ["", "LiteLLM", "apiToken.sale"], rows: [
        ["What it is", "Self-hosted proxy library and server", "Hosted multi-provider API endpoint"],
        ["Who runs the infrastructure", "You: process, uptime, upgrades", "apiToken.sale"],
        ["Where keys come from", "You open and fund each provider account", "One prepaid key covers supported Claude, GPT, Gemini and Kimi models"],
        ["Claude protocol", "Whatever upstream you configure", `Native Anthropic Messages API at ${BASE} with x-api-key`],
        ["Effect on Claude cost", "None — the upstream charges list price", "Flat 50% B2C discount on official provider rates"],
        ["Best fit", "Teams standardizing many providers behind one internal gateway", "Builders who want Claude access with nothing to operate"],
      ] },
    ] },
    { h2: "What LiteLLM gives you — and what it never will", blocks: [
      { type: "p", text: "LiteLLM solves an integration problem, not a procurement problem. It normalizes dozens of provider APIs behind one OpenAI-style call shape, and the proxy mode adds routing, retries, fallbacks, virtual keys and per-key spend tracking inside your own deployment. That is genuinely useful when several teams share one gateway." },
      { type: "p", text: "What it does not do is make the underlying tokens cheaper. Every upstream key behind the proxy is still your account, billed at list price by Anthropic, OpenAI or Google. A proxy sits between you and an invoice; it cannot shrink the invoice." },
      { type: "list", items: [
        "Provider accounts, funding and quota management stay on you.",
        "You host, patch and secure the proxy process itself.",
        "There is no discount mechanism — cost passes through unchanged.",
      ] },
    ] },
    { h2: "Where the 50% discount actually comes from", blocks: [
      { type: "p", text: "The discount is not a routing trick. apiToken.sale holds a pooled prepaid balance, meters every request against official provider rate cards — input, output and cache tokens — and then subtracts the flat 50% B2C discount before drawing from your balance. LiteLLM, by contrast, is cost-neutral: it forwards a request and the upstream charges whatever it charges." },
      { type: "p", text: "This is why the comparison is slightly unfair to both tools. LiteLLM decides where a request goes; apiToken.sale decides what a request costs. They operate on different layers, which is also why they compose well." },
      { type: "note", text: "The discount follows the key, not the client. Direct Anthropic SDK calls, curl, a coding agent, or a LiteLLM proxy in front — the charge is the same metered-and-halved amount, visible per request in the apiToken.sale dashboard." },
    ] },
    { h2: "The hybrid: LiteLLM in front of an apiToken.sale key", blocks: [
      { type: "p", text: "If you already standardized on LiteLLM's interface, you do not have to give it up to get the discount. Declare apiToken.sale as the Anthropic upstream and every Claude call through your proxy lands on the discounted endpoint:" },
      { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: ${BASE}\n      api_key: ${KEY}  # or os.environ/APITOKEN_KEY` },
      { type: "steps", items: [
        "Install the proxy as usual: pip install 'litellm[proxy]'.",
        "Save the config above. Keep the anthropic/ model prefix — that is what makes LiteLLM speak the Anthropic Messages API to the endpoint.",
        "Start it: litellm --config config.yaml. The proxy listens on http://localhost:4000 by default.",
        "Point your existing LiteLLM clients at the model name claude-opus-4-8. Requests go to router.apitoken.sale under your sk-pool key, and the 50% discount applies on the apiToken.sale side.",
      ] },
      { type: "note", text: "Keep the key out of committed files — LiteLLM's os.environ/VARIABLE syntax reads it from the environment. And note the split of duties: LiteLLM's own spend tracking shows what the proxy forwarded, but the authoritative charge is the token-level metering in your apiToken.sale dashboard." },
    ] },
    { h2: "The ops bill LiteLLM sends you", blocks: [
      { type: "p", text: "Self-hosting a proxy is a real commitment, and it is worth pricing honestly before choosing it for cost reasons. Someone has to keep the process alive, upgrade versions, rotate the master key, store every upstream provider secret, and scale the deployment when traffic grows. For a solo developer who just wants Claude in an editor or an agent loop, that overhead buys nothing." },
      { type: "p", text: `With apiToken.sale the entire integration is a base URL and a key: the native Anthropic Messages endpoint at ${BASE} with an x-api-key header, or the OpenAI-compatible lane at ${BASE}/v1 with Authorization: Bearer for tools that only speak that protocol. Claude Code, Cursor, the Anthropic SDKs and anything OpenAI-shaped connect without an adapter layer in between.` },
      { type: "link", text: "See the models covered by one key", href: "/models" },
    ] },
    { h2: "How to decide", blocks: [
      { type: "list", items: [
        "Choose apiToken.sale if you want hosted, discounted Claude access and the only change you are willing to make is a base URL and a key.",
        "Choose LiteLLM if you deliberately want to own a unified routing layer across many providers — and accept funding and operating all of it yourself.",
        "Run both if you already rely on LiteLLM's interface: put an apiToken.sale key behind it and keep the discount underneath.",
      ] },
      cta(),
    ] },
  ],
  faq: [
    { q: "Does LiteLLM discount Claude API access?", a: "No. LiteLLM routes to provider accounts you fund yourself at list price. The 50% discount comes from apiToken.sale's pooled prepaid balance, and it applies to official provider rates regardless of which client sends the request." },
    { q: "Do I need to host anything with apiToken.sale?", a: "No — it is a hosted endpoint. You change your base URL to https://router.apitoken.sale and use your sk-pool key; there is no proxy process, container or server to run." },
    { q: "Can I use LiteLLM with an apiToken.sale key?", a: "Yes. Set model: anthropic/claude-opus-4-8 with api_base: https://router.apitoken.sale and your key in litellm_params, and Claude calls through your LiteLLM proxy are billed at the discounted rate." },
    { q: "Is LiteLLM free to use?", a: "The software is open source, but free is misleading: you still pay every upstream provider at list price, plus the infrastructure and maintenance for the proxy itself. The token cost — the dominant line item — is exactly what apiToken.sale halves." },
    { q: "Which option is better for Claude Code or Cursor?", a: "Pointing the tool directly at apiToken.sale is simpler: one base URL and key, native Anthropic protocol, no extra hop. Adding LiteLLM in between only makes sense if you already run it for other reasons, like shared virtual keys across a team." },
  ],
  related: ["apitoken-vs-portkey", "apitoken-vs-openrouter", "claude-api-gateway", "anthropic-sdk-base-url"],
};
