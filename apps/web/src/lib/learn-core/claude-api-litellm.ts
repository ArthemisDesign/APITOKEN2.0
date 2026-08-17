import type { LearnArticle } from "../learn";
import { cta, BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-litellm",
  cluster: "integrate",
  title: "Use the Claude API with LiteLLM",
  h1: "Use the Claude API with LiteLLM",
  description: "Use the Claude API with LiteLLM through apiToken.sale: keep the anthropic/ prefix, set api_base to router.apitoken.sale in litellm.completion() or the proxy config, and pay 50% less per token.",
  keywords: ["claude api litellm", "litellm anthropic", "litellm claude", "litellm api_base anthropic", "litellm proxy claude", "litellm claude api key", "litellm anthropic base url", "litellm custom anthropic endpoint", "claude api through litellm proxy", "cheap claude api litellm"],
  dek: "Using the Claude API with LiteLLM through apiToken.sale comes down to one parameter: LiteLLM speaks the Anthropic Messages protocol natively, so you keep the anthropic/ model prefix and only override api_base. Same request and response shape, 50% less per token — whether you call litellm.completion() from a script or front your whole stack with the LiteLLM proxy.",
  published: "2026-07-17",
  updated: "2026-08-17",
  sections: [
    { h2: "Point litellm.completion() at the discounted endpoint", blocks: [
      { type: "p", text: "LiteLLM already implements the Anthropic Messages API, so routing Claude through apiToken.sale takes a single extra argument: keep the anthropic/ model prefix, set api_base to the gateway and pass your prepaid key. Requests and responses keep the standard Anthropic shape — only the endpoint and the per-token price change, with Claude spend at a flat 50% below list." },
      { type: "code", code: `import litellm\n\nresponse = litellm.completion(\n    model="anthropic/claude-opus-4-8",\n    api_base="${BASE}",\n    api_key="${KEY}",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n    stream=True,\n)\nfor chunk in response:\n    print(chunk.choices[0].delta.content or "", end="")` },
      { type: "p", text: "Three things do the work here. The anthropic/ prefix selects LiteLLM's Anthropic provider, so max_tokens, temperature, tools and streaming map onto the Messages API exactly as they do upstream — and max_tokens is required by that API, so set it explicitly rather than relying on defaults. api_base overrides where those requests go, per call. And api_key is your gateway key: the same sk-pool-… key works for every supported Claude model, so moving between claude-opus-4-8, claude-sonnet-5 and claude-haiku-4-5 is a string change, not a new integration." },
      { type: "note", text: "Two pitfalls bite in practice. Never strip the anthropic/ prefix: a bare claude-opus-4-8 makes LiteLLM guess the provider, and a wrong guess sends the wrong protocol or rejects the key. And read the key from the environment (api_key=os.environ[\"APITOKEN_KEY\"]) instead of pasting it into notebooks or configs that end up in git." },
    ] },
    { h2: "One LiteLLM proxy for every service that needs Claude", blocks: [
      { type: "p", text: "Direct calls are fine for a single script. Once several services, notebooks and coding agents need Claude, run LiteLLM as a proxy: one YAML file holds the endpoint and the key, every client talks to the proxy over LiteLLM's OpenAI-compatible surface, and upstream traffic stays on the Anthropic protocol." },
      { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: ${BASE}\n      api_key: ${KEY}\n  - model_name: claude-haiku-4-5\n    litellm_params:\n      model: anthropic/claude-haiku-4-5\n      api_base: ${BASE}\n      api_key: ${KEY}\nrouter_settings:\n  fallbacks:\n    - claude-opus-4-8:\n        - claude-haiku-4-5` },
      { type: "steps", items: [
        `Install the proxy extra and save the YAML above as config.yaml: pip install "litellm[proxy]".`,
        "Start the gateway: litellm --config config.yaml --port 4000.",
        `Point any OpenAI-compatible client at http://localhost:4000 with model="claude-opus-4-8" — the proxy translates the call into an Anthropic Messages request to ${BASE}.`,
        "Track spend in the apiToken.sale dashboard: usage is recorded per key with token-level detail, so one proxy key gives you one cost line for every service behind it.",
      ] },
      { type: "p", text: "The router_settings block earns its two lines: if claude-opus-4-8 errors or is unavailable, LiteLLM retries the request against claude-haiku-4-5 instead of surfacing a failure to the client. For long-running agents that hold a session open for hours, that fallback is the difference between a silent retry and a dead process." },
    ] },
    { h2: "Streaming, tool use and prompt caching survive the switch", blocks: [
      { type: "p", text: "The features that usually break behind a translation layer keep working here, because the gateway serves the native Anthropic Messages API rather than re-encoding your traffic into a different protocol. Anything LiteLLM knows how to express in Anthropic terms arrives at the model unchanged." },
      { type: "list", items: [
        "Streaming: stream=True yields the same incremental server-sent events, so token-by-token UIs and agents behave identically.",
        "Tool use: tools, tool_choice and the tool_result round-trip map onto the standard Messages blocks — function-calling agents need no rework.",
        "Prompt caching: cache_control breakpoints work as documented upstream, and cached reads are billed at the cache rates listed on the model pages.",
      ] },
      { type: "p", text: "This matters most for tools built on top of LiteLLM rather than for LiteLLM itself: many coding agents and frameworks route their Anthropic traffic through it, and they inherit the discounted endpoint from the same configuration without code changes of their own." },
    ] },
    { h2: "Mix GPT, Gemini and Kimi into the same model_list", blocks: [
      { type: "p", text: "The gateway key is multi-provider, so the proxy you just configured is not Claude-only. Add one entry per provider lane and every model draws from the same prepaid balance — no second account, no second key to rotate." },
      { type: "code", code: `# additional model_list entries\n  - model_name: gpt-5.6-terra\n    litellm_params:\n      model: openai/gpt-5.6-terra        # OpenAI-compatible lane\n      api_base: ${OPENAI_BASE}\n      api_key: ${KEY}\n  - model_name: gemini-3.6-flash\n    litellm_params:\n      model: gemini/gemini-3.6-flash     # native Gemini lane\n      api_base: ${BASE}\n      api_key: ${KEY}` },
      { type: "p", text: "Kimi models ride the same two lanes — Anthropic Messages or the universal OpenAI-compatible endpoint — so a single LiteLLM deployment can front supported Claude, GPT, Gemini and Kimi models at once. Each provider keeps the protocol LiteLLM already speaks for it; only the base URL and key point somewhere new." },
    ] },
    { h2: "What changes when you switch — and what stays identical", blocks: [
      { type: "p", text: "Switching endpoints is deliberately boring, and it is worth being precise about which parts of the stack notice and which do not." },
      { type: "table", headers: ["Layer", "What you set", "What happens"], rows: [
        ["Model IDs", "anthropic/claude-opus-4-8, anthropic/claude-sonnet-5, anthropic/claude-haiku-4-5", "Same IDs as upstream; the prefix selects the Anthropic protocol"],
        ["Endpoint", BASE, "Native Anthropic Messages API, not an OpenAI-format translation"],
        ["Features", "Streaming, tool use, prompt caching", "Behave as they do against the official endpoint"],
        ["Price", "50% below list per token", "Applies to every supported Claude model on the same prepaid balance"],
        ["Accounting", "One sk-pool-… key", "Per-key spend with token-level detail in the dashboard"],
      ] },
    ] },
    { h2: "Budget the traffic before you scale it", blocks: [
      { type: "p", text: "Billing is prepaid: you top up a balance and every request deducts its exact token cost, Claude models at the discounted rate. There is no monthly commitment to size upfront, which makes LiteLLM's per-model cost tracking a nice-to-have rather than a survival tool — the authoritative numbers live in the apiToken.sale dashboard, broken down per key with token-level detail." },
      { type: "p", text: "Before pointing a whole fleet at the proxy, run a representative day of traffic through one key and read the actual consumption off the dashboard; extrapolate from real tokens, not from list-price arithmetic. The cost calculator linked below does the same math in advance if you know your rough request mix." },
      cta(),
      { type: "link", text: "Per-model prices, including cache rates", href: "/models" },
      { type: "link", text: "Estimate your LiteLLM traffic cost in the free calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
  ],
  faq: [
    { q: "How do I set a custom Anthropic base URL in LiteLLM?", a: "Pass api_base directly to litellm.completion(), or set it under litellm_params in the proxy's model_list. LiteLLM then sends Anthropic Messages-format requests to that endpoint — for apiToken.sale, https://router.apitoken.sale." },
    { q: "Do I keep the anthropic/ model prefix when routing Claude through a gateway?", a: "Yes. Use anthropic/claude-opus-4-8 (or any supported model) so LiteLLM applies the Anthropic protocol; only the endpoint and key change, and dropping the prefix makes LiteLLM guess the provider." },
    { q: "Does LiteLLM streaming work with a custom api_base?", a: "Yes. stream=True returns the same incremental Anthropic events through the gateway, so token-by-token rendering and agent loops behave exactly as against the official endpoint." },
    { q: "Can a single LiteLLM proxy serve Claude, GPT and Gemini together?", a: "Yes. One apiToken.sale key covers supported models across Claude, GPT, Gemini and Kimi; add each provider as its own model_list entry — anthropic/ and gemini/ models against https://router.apitoken.sale, openai/ models against https://router.apitoken.sale/v1." },
    { q: "How do I fail over between Claude models in LiteLLM?", a: "Use router_settings.fallbacks in the proxy config, mapping a primary deployment to a backup — for example claude-opus-4-8 to claude-haiku-4-5. Both entries point at the same gateway and key, so the retry stays on the discounted balance." },
  ],
  related: ["claude-api-langchain", "claude-api-aider", "anthropic-sdk-base-url", "claude-api-gateway"],
};
