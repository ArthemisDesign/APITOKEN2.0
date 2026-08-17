import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-for-ai-agents",
  cluster: "explain",
  title: "Claude API for AI Agents",
  h1: "Using the Claude API for AI agents",
  description: "Build AI agents on the Claude API with apiToken.sale: one key for every model, streaming, tool use, prompt caching and a lifetime key spending limit for long-run cost control.",
  keywords: ["claude api agents", "claude ai agent api", "claude tool use", "claude agent framework", "claude api for automation"],
  dek: "Agentic workloads are token-hungry and long-running, which makes model choice, caching and cost control matter most. Here is how apiToken.sale fits agents.",
  sections: [
    { h2: "What agents need", blocks: [
      { type: "list", items: [
        "Streaming and tool use — both standard on the Anthropic Messages API.",
        "Model routing: Haiku for cheap steps, Sonnet for reasoning, Opus for the hardest.",
        "Prompt caching for repeated system prompts and tool definitions.",
        "A per-key lifetime spending limit so a runaway loop cannot spend beyond that key's cap.",
      ] },
      cta(),
    ] },
    { h2: "A cost-aware agent loop", blocks: [
      { type: "p", text: "A practical pattern: route planning and reasoning to Sonnet, cheap sub-steps and parsing to Haiku, and escalate only the hardest calls to Opus. Cache the system prompt and tool definitions so repeated context is nearly free." },
      { type: "list", items: [
        "Set a per-key lifetime spending limit so a runaway loop cannot spend beyond the cap.",
        "Stream so the agent can act on partial output.",
        "Watch token usage to tune which steps use which model.",
      ] },
    ] },
  ],
  faq: [
    { q: "Is the Claude API good for agents?", a: "Yes — with streaming, tool use, model routing and prompt caching, all on one apiToken.sale key with spend controls." },
    { q: "How do I keep agent costs down?", a: "Route cheap steps to Haiku, cache repeated context, and set a lifetime spending limit on the agent's key." },
  ],
  related: ["claude-api-streaming", "claude-api-prompt-caching", "save-tokens-on-claude-api", "claude-api-key-security"],
};
