import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-langchain",
  cluster: "integrate",
  title: "Use the Claude API with LangChain",
  h1: "Use the Claude API with LangChain",
  description: "Connect LangChain to Claude through apiToken.sale: point ChatAnthropic at router.apitoken.sale, keep the same model IDs, and pay 50% less per token.",
  keywords: ["claude api langchain", "langchain anthropic", "langchain claude", "chatanthropic base url", "langchain claude api key", "langchain anthropic_api_url"],
  dek: "LangChain's Anthropic integration accepts a custom API URL, so your chains and agents can run on Claude through apiToken.sale with a two-line change — same models, lower token price.",
  published: "2026-07-17",
  updated: "2026-07-17",
  sections: [
    { h2: "Point ChatAnthropic at the gateway", blocks: [
      { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nllm = ChatAnthropic(\n    model="claude-opus-4-8",\n    anthropic_api_url="${BASE}",\n    anthropic_api_key="${KEY}",\n)\nprint(llm.invoke("Hello").content)` },
      { type: "p", text: "That is the whole integration: the same langchain-anthropic package, the same model IDs, the same streaming and tool-calling — only the endpoint and the price change." },
      cta(),
    ] },
    { h2: "Or configure via environment variables", blocks: [
      { type: "code", code: `export ANTHROPIC_API_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}` },
      { type: "p", text: "With the environment set, ChatAnthropic picks up both values automatically, so shared codebases need no code change at all." },
    ] },
    { h2: "What works", blocks: [
      { type: "list", items: [
        "Chains, agents and LangGraph workflows — the protocol is unchanged.",
        "Streaming, tool calling and structured output through the standard integration.",
        "Every supported Claude model (Opus, Sonnet, Haiku) on one key and balance.",
      ] },
    ] },
  ],
  faq: [
    { q: "Does LangChain work with a custom Claude API endpoint?", a: "Yes. ChatAnthropic accepts anthropic_api_url (or the ANTHROPIC_API_URL environment variable), so you can point it at https://router.apitoken.sale and keep everything else unchanged." },
    { q: "Do LangChain agents and tool calling still work?", a: "Yes — the gateway serves the standard Anthropic Messages API, so tool calling, streaming and LangGraph agents behave exactly as with the official endpoint." },
    { q: "Which models can I use from LangChain?", a: "All supported Claude models — claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 and more — on the same key and prepaid balance." },
  ],
  related: ["anthropic-sdk-base-url", "claude-api-litellm", "claude-api-quick-setup", "cheapest-claude-api"],
};
