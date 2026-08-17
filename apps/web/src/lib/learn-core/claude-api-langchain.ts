import type { LearnArticle } from "../learn";
import { cta, BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-langchain",
  cluster: "integrate",
  title: "Use the Claude API with LangChain",
  h1: "Use the Claude API with LangChain",
  description: "Connect LangChain to Claude through apiToken.sale: point ChatAnthropic at router.apitoken.sale, keep the same model IDs, and pay 50% less per token.",
  keywords: ["claude api langchain", "langchain anthropic", "langchain claude", "chatanthropic base url", "langchain claude api key", "langchain anthropic_api_url", "langchain custom anthropic endpoint", "langgraph claude api", "chatanthropic streaming", "langchain claude cheaper"],
  dek: "The Claude API works with LangChain out of the box, and ChatAnthropic accepts a custom API URL — so your chains and agents can run on Claude through apiToken.sale after a two-line change. Same langchain-anthropic package, same model IDs, same streaming and tool calling; only the endpoint and the token price change.",
  published: "2026-07-17",
  updated: "2026-08-17",
  sections: [
    { h2: "Point ChatAnthropic at router.apitoken.sale", blocks: [
      { type: "p", text: "LangChain's Anthropic integration takes a custom API URL, so connecting the Claude API to LangChain through apiToken.sale is exactly two constructor arguments: anthropic_api_url and anthropic_api_key. Prompts, output parsers, callbacks and retry logic in your existing chains stay untouched." },
      { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nllm = ChatAnthropic(\n    model="claude-opus-4-8",\n    anthropic_api_url="${BASE}",\n    anthropic_api_key="${KEY}",\n)\nprint(llm.invoke("Hello").content)` },
      { type: "note", text: "Pass the router root exactly as shown: no trailing slash, no /v1 suffix. The underlying Anthropic client appends /v1/messages itself, and a doubled path is the most common cause of a 404 on an otherwise correct setup." },
      { type: "p", text: "One argument worth setting explicitly is max_tokens. ChatAnthropic defaults to 1024 output tokens, which silently truncates long answers — raise it for summarization or code-generation chains. Sampling parameters like temperature and top_p pass through unchanged, as do system prompts and stop sequences." },
      cta(),
    ] },
    { h2: "Set it once with environment variables", blocks: [
      { type: "p", text: "If you share a codebase with people on the official endpoint — or run notebooks where editing source is awkward — skip the constructor arguments entirely. ChatAnthropic reads both values from the environment, so a checked-in project needs zero code changes." },
      { type: "code", code: `export ANTHROPIC_API_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}` },
      { type: "steps", items: [
        "Install the partner package: pip install -U langchain-anthropic. LangChain ships Anthropic support there, not in langchain-core.",
        "Generate a key in the apiToken.sale dashboard — it starts with sk-pool- and works across supported Claude, GPT, Gemini and Kimi models.",
        "Export ANTHROPIC_API_URL and ANTHROPIC_API_KEY as above (or put them in the .env file your runner loads).",
        "Instantiate ChatAnthropic(model=\"claude-sonnet-5\") with no other arguments and run one invoke() to confirm a normal response.",
      ] },
      { type: "p", text: "Explicit constructor arguments win over environment variables, so a local override never leaks into the shared configuration. The env approach also keeps the key out of git history — treat sk-pool-… like any other secret: .env files stay uncommitted and CI gets the value from its secret store." },
    ] },
    { h2: "Streaming, tool calling and LangGraph stay intact", blocks: [
      { type: "p", text: "The gateway serves the standard Anthropic Messages API, and LangChain talks to it through the official client. Everything built on that protocol — SSE streaming, tool-use blocks, structured output — behaves exactly as it does against api.anthropic.com. That includes with_structured_output(), which LangChain implements on top of tool calling, and .astream_events() for token-level callbacks in async apps." },
      { type: "code", code: `from langchain_anthropic import ChatAnthropic\nfrom langchain_core.tools import tool\n\n@tool\ndef get_weather(city: str) -> str:\n    """Return the current weather for a city."""\n    return f"Sunny in {city}"\n\nllm = ChatAnthropic(model="claude-sonnet-5")  # env vars supply URL and key\nllm_with_tools = llm.bind_tools([get_weather])\n\nfor chunk in llm_with_tools.stream("What is the weather in Paris?"):\n    print(chunk.content, end="")` },
      { type: "p", text: "LangGraph agents inherit the same setup, because a graph node just invokes a chat model. Point the model at the router once and every agent, supervisor and sub-graph built on it follows — there is no LangGraph-specific configuration to redo." },
      { type: "note", text: "Token accounting keeps working too: each AIMessage still carries usage_metadata with input and output token counts, because the gateway returns the standard Anthropic usage object. LangSmith traces and custom callbacks that read usage_metadata need no changes." },
    ] },
    { h2: "What changes and what does not", blocks: [
      { type: "p", text: "Before migrating a production app, it helps to see the full delta in one place. The short version: your code, your models and your LangChain features stay put — the endpoint, the key and the per-token price are the only moving parts." },
      { type: "table", headers: ["Concern", "Through apiToken.sale"], rows: [
        ["Model IDs", "Unchanged — claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 and the rest of the catalog"],
        ["Protocol", "Unchanged — Anthropic Messages API via the official client"],
        ["Streaming and tool calling", "Unchanged — SSE chunks and tool-use blocks as usual"],
        ["Chains, agents, LangGraph", "Unchanged — no code edits beyond URL and key"],
        ["Price per token", "50% less on the same models"],
        ["API key", "One sk-pool-… key for supported Claude, GPT, Gemini and Kimi models"],
        ["Billing", "Prepaid balance with per-key spend and token detail in the dashboard"],
      ] },
      { type: "link", text: "Check the full list of supported Claude models and prices", href: "/models" },
    ] },
    { h2: "Pick the right Claude model for each node", blocks: [
      { type: "p", text: "Because switching models is a one-argument change, treat model choice as a per-node decision instead of a global one. A router chain that classifies intent does not need the same tier as the node that writes the final answer." },
      { type: "list", items: [
        "claude-haiku-4-5 — the fast, inexpensive tier: classification, routing, extraction and other high-volume steps.",
        "claude-sonnet-5 — the balanced default for most production chains, RAG pipelines and coding agents.",
        "claude-opus-4-8 — the top reasoning tier; reserve it for hard analysis, long documents and agent planning steps.",
      ] },
      { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nfast = ChatAnthropic(model="claude-haiku-4-5")      # routing, extraction\nbalanced = ChatAnthropic(model="claude-sonnet-5")   # default nodes\ndeep = ChatAnthropic(model="claude-opus-4-8")       # planning, hard analysis\n\nrouter_chain = router_prompt | fast\nanswer_chain = answer_prompt | balanced | StrOutputParser()` },
      { type: "p", text: "All three instances share the same environment-supplied URL and key, and every call bills to one prepaid balance. That makes tier experiments cheap: swap the model string, rerun your evaluation set, keep what wins." },
      { type: "note", text: "Prototype on Sonnet, then downgrade the simple nodes to Haiku and escalate only the hard ones to Opus. With prepaid per-token billing, a mixed-tier chain costs noticeably less than running everything on the flagship." },
      { type: "link", text: "Estimate a mixed-model chain with the cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Troubleshooting the connection", blocks: [
      { type: "p", text: "Only the endpoint and the key change, so almost every failure is one of three configuration mistakes — not a LangChain problem. Work through them in order before touching chain code." },
      { type: "list", items: [
        "401 Unauthorized — the key is missing or mistyped, or the environment variable never reached the process. Print os.environ in the same interpreter to confirm, and remember constructor arguments override env vars.",
        "404 Not Found — the URL carries an extra /v1 or a trailing path. Use the bare router root https://router.apitoken.sale.",
        "Model not found — re-check the ID against the catalog at /models; the IDs here are the same ones Anthropic publishes.",
      ] },
      { type: "p", text: "If you are unsure whether the gateway or your chain is at fault, swap the URL back to the official endpoint for one run. Identical behavior means the bug is in the chain; a difference narrows it to configuration." },
      { type: "note", text: "For transient 429 or 5xx responses you do not need custom logic: ChatAnthropic retries twice with backoff by default (tune with max_retries). Long-running agents should still set an explicit timeout in seconds rather than relying on the client default." },
    ] },
  ],
  faq: [
    { q: "Does LangChain work with a custom Claude API endpoint?", a: "Yes. ChatAnthropic accepts anthropic_api_url (or the ANTHROPIC_API_URL environment variable), so you can point it at https://router.apitoken.sale and keep everything else — package, model IDs, chain code — unchanged." },
    { q: "How do I set the LangChain Anthropic base URL without changing code?", a: "Export ANTHROPIC_API_URL=https://router.apitoken.sale and ANTHROPIC_API_KEY=sk-pool-… before running your script. ChatAnthropic picks both up automatically, so shared repositories need no edits at all." },
    { q: "Do streaming and tool calling still work through apiToken.sale?", a: "Yes. The gateway serves the standard Anthropic Messages API, so .stream(), bind_tools(), structured output and LangGraph agents behave exactly as with the official endpoint." },
    { q: "Which Claude models can I call from LangChain?", a: "All supported Claude models — claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 and more — on the same key and prepaid balance, at 50% less per token." },
    { q: "Can I use ChatOpenAI instead of ChatAnthropic for Claude?", a: `Yes. The router also exposes an OpenAI-compatible lane at ${OPENAI_BASE}, so ChatOpenAI(base_url="${OPENAI_BASE}", api_key="${KEY}") reaches the same Claude models with the same key — handy when a framework only speaks the OpenAI protocol.` },
    { q: "Do I need a separate key for GPT, Gemini or Kimi in LangChain?", a: "No. The same sk-pool-… key works across supported Claude, GPT, Gemini and Kimi models, so a multi-provider LangChain app can share one key and one prepaid balance." },
  ],
  related: ["anthropic-sdk-base-url", "claude-api-litellm", "claude-api-quick-setup", "cheapest-claude-api"],
};
