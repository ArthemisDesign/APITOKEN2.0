import type { LearnArticle } from "../learn";
import { cta, BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-for-ai-agents",
  cluster: "explain",
  title: "Claude API for AI Agents",
  h1: "Using the Claude API for AI agents",
  description: "Build AI agents on the Claude API: one key for every model, tool use and streaming, prompt caching, and a lifetime spending limit to stop runaway loops.",
  keywords: ["claude api agents", "claude ai agent api", "claude tool use api", "build ai agent with claude", "claude agent loop cost", "claude api model routing", "claude prompt caching for agents", "claude api streaming agents", "claude api spend limit", "multi-agent claude api"],
  dek: "The Claude API is a strong foundation for agents: tool use and streaming are first-class, and the model lineup maps cleanly onto the steps of an agent loop. The catch is economics — a loop makes dozens of calls per task, so model routing, caching and a hard spend cap decide whether the run is viable. This guide covers all three on the Claude API for agents through apiToken.sale.",
  updated: "2026-08-17",
  sections: [
    { h2: "Why agent loops burn tokens faster than chat", blocks: [
      { type: "p", text: "Yes, the Claude API works well for AI agents — tool use, streaming and prompt caching are standard parts of the Anthropic Messages API, and all of it is reachable through a single apiToken.sale key. The difference from a chatbot is volume. A chat user sends one message and reads one reply. An agent plans, calls a tool, reads the result, re-plans and repeats — easily dozens of model calls for a single user-visible task, each carrying the full conversation so far." },
      { type: "p", text: "That profile changes what matters. Latency per call is less important than cost per completed task. Repeated context — the system prompt, the tool definitions, the accumulating tool results — dominates the token count, not the final answer. Get three things right and agent economics work: route each step to the cheapest model that can do it, cache everything that repeats, and cap what a runaway loop can spend." },
    ] },
    { h2: "Route each step of the loop to the right model", blocks: [
      { type: "p", text: "Treating the whole loop as one model call is the most expensive mistake in agent design. The planning step needs strong reasoning; the step that extracts a URL from a tool result does not. Anthropic's lineup maps directly onto this: Haiku for cheap mechanical steps, Sonnet for the reasoning core, Opus for the rare calls where the first two fail. On apiToken.sale all three sit on the same key and balance, so switching tiers is one string in the request — no extra accounts, no extra billing relationships." },
      { type: "table", headers: ["Loop step", "Model", "Why"], rows: [
        ["Planning, decomposition, self-critique", "claude-sonnet-5", "Best balance of reasoning quality and cost; the default workhorse"],
        ["Parsing, classification, extraction, routing", "claude-haiku-4-5", "Cheapest tier; these steps are high-volume and low-difficulty"],
        ["Hardest calls after a Sonnet attempt fails", "claude-opus-4-8", "Escalation only — reserve it for the steps that actually need it"],
      ] },
      { type: "p", text: "A practical escalation pattern: run the step on Sonnet, and only if the output fails validation (malformed JSON, a rejected plan, a failed test) retry that one step on Opus. Most loops never escalate, and you pay Opus rates only where they buy something." },
    ] },
    { h2: "The smallest agentic call: tool use over SSE", blocks: [
      { type: "p", text: "An agent step is a normal Messages API request with two additions: a tools array describing what the model may call, and stream set to true so you can act on partial output. The model answers with a tool_use block and stop_reason \"tool_use\"; your code executes the tool, appends a tool_result message, and calls the API again. That round trip is the entire agent loop — everything else is orchestration." },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "system": "You are a research agent. Use tools, then answer.",\n    "tools": [{\n      "name": "web_search",\n      "description": "Search the web",\n      "input_schema": {"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}\n    }],\n    "messages": [{"role":"user","content":"Find the latest Anthropic model"}]\n  }'` },
      { type: "p", text: "The endpoint is the native Anthropic Messages API, so the official Anthropic SDKs — and every agent framework that speaks this protocol — work unchanged with a different base URL and key. Streaming arrives as standard server-sent events, and streamed requests are billed exactly like non-streamed ones: by input and output tokens." },
    ] },
    { h2: "Cache the static parts of every request", blocks: [
      { type: "p", text: "In a twenty-step loop, the system prompt and tool definitions are sent twenty times. Prompt caching turns that repeated context from a recurring cost into a nearly free one: mark the stable prefix with a cache_control breakpoint, and cache reads on subsequent calls cost a fraction of fresh input tokens. The cache entry lives for a fixed short window (five minutes by default) that refreshes on every hit — exactly the access pattern of an active agent." },
      { type: "p", text: "Order matters. Put the most stable content first — system prompt, then tool definitions, then the oldest conversation history — and never insert volatile content (timestamps, request IDs) ahead of a breakpoint, or every call becomes a cache write instead of a cache read." },
      { type: "note", text: "Caching compounds with the flat 50% discount apiToken.sale applies to official token pricing: caching reduces the token count, the discount reduces the price per token." },
      { type: "link", text: "Estimate a loop's real cost before it runs unattended", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Bound the blast radius of a runaway loop", blocks: [
      { type: "p", text: "Every agent eventually hits a loop that cannot converge — a tool that keeps returning errors the model keeps retrying. Client-side guards (a max-iteration counter, a per-task token budget, a wall-clock timeout) are your first line, but they live in your process and die with bugs in your process. The second line belongs on the key itself." },
      { type: "steps", items: [
        "Create a separate named key per agent in the apiToken.sale dashboard — never share one key across agents or with humans.",
        `Point the agent at ${BASE} with that key in the x-api-key header, exactly like a normal Messages API client.`,
        "Set a lifetime spending limit on the key: once cumulative spend on that key hits the cap, further requests are refused, so a broken loop cannot spend beyond it.",
        "Set an expiration date when the agent is temporary — a demo, a CI job, a contractor's prototype — so access ends automatically.",
        "Watch token-level usage per key in the dashboard; a step that suddenly dominates the bill is usually a routing or caching bug, not a price problem.",
      ] },
    ] },
    { h2: "Mix providers inside one agent", blocks: [
      { type: "p", text: "Agent steps do not all have to be Claude. The same apiToken.sale key also serves supported GPT, Gemini and Kimi models, so a loop can draft with Claude, run a cheap classification step on a lighter model from another family, or compare answers across providers for a verification step. Anthropic-native calls keep the Messages shape shown above; GPT models run through the OpenAI-compatible lane with an Authorization: Bearer header." },
      { type: "code", code: `curl ${OPENAI_BASE}/chat/completions \\\n  -H "Authorization: Bearer ${KEY}" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "gpt-5.5",\n    "messages": [{"role":"user","content":"Classify: is this tool result an error?"}]\n  }'` },
      { type: "p", text: "All of it lands on one prepaid balance with the same per-token discount, which is what makes heterogeneous loops practical: no per-provider accounts, no separate budgets to reconcile." },
    ] },
    { h2: "What a well-tuned agent looks like on the bill", blocks: [
      { type: "list", items: [
        "Most calls are Haiku or Sonnet; Opus appears only on genuine escalations.",
        "Cache reads dominate the input tokens on every call after the first.",
        "Streaming is on, so the orchestrator can abort early when a tool call arrives malformed.",
        "Every key has a lifetime spending limit and a name that tells you which agent owns it.",
        "The prepaid balance never expires, so a quiet month costs nothing.",
      ] },
      { type: "link", text: "Streaming mechanics in depth: SSE events, early aborts, billing parity", href: "/docs/learn/claude-api-streaming" },
      cta(),
    ] },
  ],
  faq: [
    { q: "Is the Claude API good for building AI agents?", a: "Yes. Tool use and streaming are first-class in the Anthropic Messages API, and the Haiku/Sonnet/Opus tiers map cleanly onto the steps of an agent loop — all reachable through one apiToken.sale key." },
    { q: "Which Claude model should an agent use by default?", a: "claude-sonnet-5 for planning and reasoning, claude-haiku-4-5 for high-volume mechanical steps like parsing and classification, and claude-opus-4-8 only as an escalation for calls where Sonnet fails validation." },
    { q: "How do I keep an agent loop from overspending?", a: "Combine client-side guards (iteration and token caps) with a lifetime spending limit on the agent's apiToken.sale key, which hard-stops spend at the cap; add an expiration date for temporary agents." },
    { q: "Does tool use work through apiToken.sale?", a: "Yes — it is the native Anthropic Messages API at router.apitoken.sale, so the standard tool_use/tool_result round trip and the official SDKs work with just a different base URL and key." },
    { q: "Should agents stream responses?", a: "Usually yes: streaming lets the orchestrator act on partial output and abort early, and streamed requests are billed identically to non-streamed ones by input and output tokens." },
    { q: "Can one agent mix Claude with GPT, Gemini or Kimi models?", a: "Yes — the same key and prepaid balance cover all four families. Claude uses the Anthropic Messages endpoint; GPT uses the OpenAI-compatible lane with an Authorization: Bearer header." },
  ],
  related: ["claude-api-streaming", "claude-api-prompt-caching", "save-tokens-on-claude-api", "claude-api-key-security"],
};
