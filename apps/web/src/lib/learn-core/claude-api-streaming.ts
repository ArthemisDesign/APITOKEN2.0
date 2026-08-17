import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-streaming",
  cluster: "explain",
  title: "Streaming with the Claude API",
  h1: "Streaming responses from the Claude API",
  description: "How to stream Claude responses on apiToken.sale for responsive coding agents and UIs. Same Anthropic SSE format, billed the same as non-streaming.",
  keywords: ["claude api streaming", "claude sse", "stream claude responses", "anthropic streaming api", "claude api real-time", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
  dek: "Streaming sends tokens as they are generated, which makes agents and chat UIs feel instant. apiToken.sale supports the standard Anthropic streaming format.",
  sections: [
    { h2: "How to stream", blocks: [
      { type: "p", text: "Set \"stream\": true in your request (or use the SDK's streaming helper). The gateway returns standard Anthropic server-sent events." },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
    ] },
    { h2: "Billing is identical", blocks: [
      { type: "p", text: "Streaming and non-streaming requests are billed the same way — by input and output tokens — so you lose nothing by streaming." },
      cta(),
    ] },
    { h2: "When streaming is worth it", blocks: [
      { type: "list", items: [
        "Chat and coding UIs where users watch the answer appear.",
        "Long generations, so you can render or act on partial output early.",
        "Agents that stop as soon as a tool call is emitted.",
      ] },
      { type: "p", text: "For short batch jobs, non-streaming is simpler; the cost is identical either way." },
    ] },
  ],
  faq: [
    { q: "Does apiToken.sale support streaming?", a: "Yes — the standard Anthropic SSE streaming format works for coding agents, IDEs and production calls." },
    { q: "Does streaming cost more?", a: "No. Streaming and non-streaming requests are billed identically by tokens." },
  ],
  related: ["claude-api-quick-setup", "claude-api-rate-limits", "anthropic-sdk-base-url", "claude-api-for-ai-agents"],
};
